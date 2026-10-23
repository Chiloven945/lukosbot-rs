use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    ArgType, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, InputSpec, OptionSpec, PositionalSpec, SyntaxLine, SyntaxPart,
    UsageMetadata,
};
use lukosbot_integrations::{
    IpData, IpLookupService, IpProviderId, IpQueryResult, resolve_provider_selection,
};

use super::error::map_integration_error;
use super::format::{strip_controls, truncate_reply};

pub struct IpCommand {
    service: Arc<dyn IpLookupService>,
}

impl IpCommand {
    pub fn new(service: Arc<dyn IpLookupService>) -> Self {
        Self { service }
    }

    pub fn into_definition(self) -> CommandDefinition {
        let args = ArgsSpec {
            positionals: vec![
                PositionalSpec::new("ip", ArgType::String).description("IP 地址（IPv4 / IPv6）"),
            ],
            options: vec![
                OptionSpec::new(
                    "providers",
                    ["-p", "--provider", "--providers"],
                    ArgType::String,
                )
                .repeatable()
                .split_by(",")
                .description("指定数据源；多个用逗号分隔"),
            ],
        };

        CommandDefinition::new("ip", "查询 IP 信息", InputSpec::Args(args), Arc::new(self)).usage(
            UsageMetadata {
                syntaxes: vec![
                    SyntaxLine::described(
                        "按默认数据源优先级查询",
                        [SyntaxPart::Argument {
                            name: "ip_address".to_owned(),
                            optional: false,
                        }],
                    ),
                    SyntaxLine::described(
                        "指定数据源",
                        [
                            SyntaxPart::Argument {
                                name: "ip_address".to_owned(),
                                optional: false,
                            },
                            SyntaxPart::Option {
                                names: vec!["-p".to_owned()],
                                value_name: Some("provider1,provider2".to_owned()),
                                optional: true,
                            },
                        ],
                    ),
                ],
                examples: vec![
                    "ip 1.1.1.1".to_owned(),
                    "ip 2606:4700:4700::1111".to_owned(),
                    "ip --provider=ipquery 1.1.1.1".to_owned(),
                    "ip --provider=ipquery,ipsb 1.1.1.1".to_owned(),
                ],
                notes: vec!["可用数据源：ipquery、ipsb。".to_owned()],
            },
        )
    }
}

#[async_trait]
impl CommandExecutor for IpCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        if invocation.path.as_slice() != ["ip"] {
            return Err(CommandError::internal("invalid ip command path"));
        }

        let raw_ip = invocation
            .args
            .get::<String>("ip")
            .map_err(CommandError::internal)?;
        let ip = raw_ip.trim().parse::<IpAddr>().map_err(|_| {
            CommandError::UserInput("IP 地址格式无效；请输入 IPv4 或 IPv6 地址。".to_owned())
        })?;

        let requested = invocation
            .args
            .get_opt::<Vec<String>>("providers")
            .map_err(CommandError::internal)?
            .unwrap_or_default();
        let providers = resolve_provider_selection(&requested).map_err(|unknown| {
            CommandError::UserInput(format!(
                "未知 IP 数据源：{}。可用数据源：{}",
                unknown.join("、"),
                available_providers()
            ))
        })?;
        if !requested.is_empty() && providers.is_empty() {
            return Err(CommandError::UserInput(
                "请指定至少一个 IP 数据源。".to_owned(),
            ));
        }

        let result = self
            .service
            .query(ip, &providers)
            .await
            .map_err(map_query_error)?;
        context.reply_text(format_result(&result)).await?;
        Ok(())
    }
}

fn map_query_error(error: lukosbot_integrations::IntegrationError) -> CommandError {
    if let lukosbot_integrations::IntegrationError::AllProvidersFailed { providers } = &error {
        tracing::warn!(
            category = error.category(),
            tried = ?providers,
            "all IP providers failed"
        );
        return CommandError::UserInput(format!(
            "IP 查询失败，所有数据源均不可用。已尝试：{}",
            providers
                .iter()
                .map(|provider| provider.id())
                .collect::<Vec<_>>()
                .join("、")
        ));
    }

    map_integration_error(error, "未找到该 IP 的信息。", "IP 查询参数无效。")
}

fn format_result(result: &IpQueryResult) -> String {
    let mut text = format_data(&result.data, result.provider);
    if !result.failed_providers.is_empty() {
        text.push_str(&format!(
            "\n\n提示：部分数据源暂时不可用，已自动切换到 {}。不可用数据源：{}",
            result.provider.id(),
            result
                .failed_providers
                .iter()
                .map(|provider| provider.id())
                .collect::<Vec<_>>()
                .join("、")
        ));
    }
    truncate_reply(&text)
}

fn format_data(data: &IpData, provider: IpProviderId) -> String {
    let mut lines = vec![format!("IP 地址：{}", data.ip)];

    push_pair(&mut lines, "国家/地区", &data.country, &data.country_code);
    push_pair(&mut lines, "区域", &data.region, &data.region_code);
    push_pair(&mut lines, "城市", &data.city, &data.postal_code);

    if let (Some(latitude), Some(longitude)) = (data.latitude, data.longitude) {
        lines.push(format!("位置：{latitude:.6}, {longitude:.6}"));
    }

    push_line(&mut lines, "时区", &data.timezone);
    push_line(&mut lines, "组织", &data.organization);
    push_line(&mut lines, "ISP", &data.isp);
    push_line(&mut lines, "ASN", &data.asn);

    if let Some(risk) = &data.risk {
        let mut flags = Vec::new();
        if risk.mobile == Some(true) {
            flags.push("移动网络");
        }
        if risk.vpn == Some(true) {
            flags.push("VPN");
        }
        if risk.tor == Some(true) {
            flags.push("Tor");
        }
        if risk.proxy == Some(true) {
            flags.push("Proxy");
        }
        if risk.datacenter == Some(true) {
            flags.push("机房");
        }
        if !flags.is_empty() {
            lines.push(format!("风险标记：{}", flags.join(" / ")));
        }
        if let Some(score) = risk.score {
            lines.push(format!("风险分：{score}/100"));
        }
    }

    lines.push(format!("数据源：{}", provider.id()));
    lines.join("\n")
}

fn push_pair(lines: &mut Vec<String>, label: &str, main: &Option<String>, extra: &Option<String>) {
    let Some(main) = main
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let mut line = format!("{label}：{}", strip_controls(main));
    if let Some(extra) = extra
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        line.push_str(&format!(" ({})", strip_controls(extra)));
    }
    lines.push(line);
}

fn push_line(lines: &mut Vec<String>, label: &str, value: &Option<String>) {
    if let Some(value) = value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("{label}：{}", strip_controls(value)));
    }
}

fn available_providers() -> String {
    IpProviderId::ALL
        .iter()
        .map(|provider| format!("{}(优先级 {})", provider.id(), provider.priority()))
        .collect::<Vec<_>>()
        .join("、")
}

pub fn definition(service: Arc<dyn IpLookupService>) -> CommandDefinition {
    IpCommand::new(service).into_definition()
}
