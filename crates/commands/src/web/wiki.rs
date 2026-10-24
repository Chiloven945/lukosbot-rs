use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    CommandContext, CommandDefinition, CommandError, CommandExecutor, CommandInvocation,
    CommandNode, InputSpec, UsageMetadata,
};
use lukosbot_domain::{MediaRef, MediaSource, OutboundMessage, OutboundPart};
use lukosbot_integrations::{
    IntegrationError, RenderedDocument, WikiService, WikiSite, WikiTarget,
};

use crate::integrations::error::map_integration_error;
use crate::integrations::format::truncate_reply;

pub struct WikiCommand {
    service: Arc<dyn WikiService>,
    site: WikiSite,
    name: &'static str,
}

impl WikiCommand {
    pub fn wikipedia(service: Arc<dyn WikiService>) -> Self {
        Self {
            service,
            site: WikiSite::Wikipedia,
            name: "wiki",
        }
    }

    pub fn minecraft(service: Arc<dyn WikiService>) -> Self {
        Self {
            service,
            site: WikiSite::MinecraftWiki,
            name: "mcwiki",
        }
    }

    pub fn into_definition(self) -> CommandDefinition {
        let site = self.site;
        let name = self.name;
        let command: Arc<dyn CommandExecutor> = Arc::new(self);
        let mut definition = CommandDefinition::new(
            name,
            match site {
                WikiSite::Wikipedia => "查询维基百科，支持截图和导出 Markdown",
                WikiSite::MinecraftWiki => "查询 Minecraft Wiki，支持简介、截图和导出 Markdown",
            },
            InputSpec::Raw {
                name: "target".to_owned(),
                required: false,
            },
            Arc::clone(&command),
        )
        .child(CommandNode::leaf(
            "md",
            "导出 Markdown",
            InputSpec::Raw {
                name: "target".to_owned(),
                required: true,
            },
            Arc::clone(&command),
        ));

        if site == WikiSite::MinecraftWiki {
            definition = definition.child(CommandNode::leaf(
                "ss",
                "生成条目截图",
                InputSpec::Raw {
                    name: "target".to_owned(),
                    required: true,
                },
                command,
            ));
        }

        definition.usage(UsageMetadata {
            examples: vec![
                match site {
                    WikiSite::Wikipedia => "wiki Java".to_owned(),
                    WikiSite::MinecraftWiki => "mcwiki 僵尸猪灵".to_owned(),
                },
                match site {
                    WikiSite::Wikipedia => "wiki md en:Albert_Einstein".to_owned(),
                    WikiSite::MinecraftWiki => "mcwiki ss 下界合金".to_owned(),
                },
            ],
            notes: vec!["条目名支持 `en:` 前缀指定语言。".to_owned()],
            ..UsageMetadata::default()
        })
    }

    async fn summary(&self, context: &CommandContext<'_>, raw: &str) -> Result<(), CommandError> {
        let target = self.target(raw)?;
        let summary = self.service.summary(&target).await.map_err(|error| {
            map_integration_error(error, "未找到该条目。", "条目链接或名称无效。")
        })?;
        if summary.title.trim().is_empty() {
            return Err(CommandError::UserInput(
                "未获取到有效内容，请检查条目是否存在。".to_owned(),
            ));
        }
        let mut text = summary.title;
        if !summary.lead.is_empty() {
            text.push('\n');
            text.push_str(&summary.lead);
        }
        context.reply_text(truncate_reply(&text)).await?;
        Ok(())
    }

    async fn markdown(&self, context: &CommandContext<'_>, raw: &str) -> Result<(), CommandError> {
        let target = self.target(raw)?;
        let document = self
            .service
            .export_markdown(&target)
            .await
            .map_err(|error| {
                map_integration_error(error, "未找到该条目。", "条目链接或名称无效。")
            })?;
        self.reply_document(context, document, "已转换为 Markdown。")
            .await
    }

    async fn screenshot(
        &self,
        context: &CommandContext<'_>,
        raw: &str,
    ) -> Result<(), CommandError> {
        let target = self.target(raw)?;
        let document = self
            .service
            .screenshot_intro(&target)
            .await
            .map_err(|error| match error {
                IntegrationError::NotConfigured { .. } => CommandError::UserInput(
                    "Wiki 截图未启用。请设置 web.browser.enabled 并提供 WebDriver，或使用 /wiki md 导出 Markdown。"
                        .to_owned(),
                ),
                error => {
                    map_integration_error(error, "未找到该条目。", "条目链接或名称无效。")
                }
            })?;
        self.reply_image(
            context,
            document,
            if self.site == WikiSite::MinecraftWiki {
                Some("截图如下。")
            } else {
                None
            },
        )
        .await
    }

    fn target(&self, raw: &str) -> Result<WikiTarget, CommandError> {
        WikiTarget::parse(self.site, raw).map_err(|error| match error {
            IntegrationError::InvalidInput { .. } => CommandError::UserInput(format!(
                "仅支持{}链接或条目名。",
                if self.site == WikiSite::Wikipedia {
                    "Wikipedia"
                } else {
                    "Minecraft Wiki"
                }
            )),
            error => map_integration_error(error, "未找到该条目。", "条目链接或名称无效。"),
        })
    }

    async fn reply_document(
        &self,
        context: &CommandContext<'_>,
        document: RenderedDocument,
        caption: &str,
    ) -> Result<(), CommandError> {
        context
            .reply_message(OutboundMessage {
                parts: vec![OutboundPart::File {
                    media: MediaRef {
                        source: MediaSource::Bytes(document.bytes),
                        file_name: Some(document.file_name),
                        mime_type: Some(document.mime_type),
                        size_bytes: None,
                    },
                    caption: Some(caption.to_owned()),
                }],
            })
            .await?;
        Ok(())
    }

    async fn reply_image(
        &self,
        context: &CommandContext<'_>,
        document: RenderedDocument,
        caption: Option<&str>,
    ) -> Result<(), CommandError> {
        context
            .reply_message(OutboundMessage {
                parts: vec![OutboundPart::Image {
                    media: MediaRef {
                        source: MediaSource::Bytes(document.bytes),
                        file_name: Some(document.file_name),
                        mime_type: Some(document.mime_type),
                        size_bytes: None,
                    },
                    caption: caption.map(str::to_owned),
                }],
            })
            .await?;
        Ok(())
    }
}

#[async_trait]
impl CommandExecutor for WikiCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        let path = invocation
            .path
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        match path.as_slice() {
            [root] if *root == self.name => {
                if invocation.raw_tail.trim().is_empty() {
                    context
                        .reply_text(format!("用法：{} <条目或链接>", self.name))
                        .await?;
                    Ok(())
                } else if self.site == WikiSite::MinecraftWiki {
                    self.summary(context, &invocation.raw_tail).await
                } else {
                    self.screenshot(context, &invocation.raw_tail).await
                }
            }
            [root, "md"] if *root == self.name => {
                self.markdown(context, &invocation.raw_tail).await
            }
            [root, "ss"] if *root == self.name && self.site == WikiSite::MinecraftWiki => {
                self.screenshot(context, &invocation.raw_tail).await
            }
            _ => Err(CommandError::internal("invalid wiki command path")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_targets_for_both_sites() {
        assert!(WikiTarget::parse(WikiSite::Wikipedia, "en:Java").is_ok());
        assert!(WikiTarget::parse(WikiSite::MinecraftWiki, "僵尸猪灵").is_ok());
    }
}
