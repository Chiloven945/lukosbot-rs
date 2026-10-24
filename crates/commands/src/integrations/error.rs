use lukosbot_command_engine::CommandError;
use lukosbot_integrations::IntegrationError;

/// Converts a service failure into a bounded Chinese message.
///
/// Provider bodies and source errors are deliberately not echoed. `not_found` and `invalid`
/// provide command-specific wording for the two categories commands can explain precisely.
pub(crate) fn map_integration_error(
    error: IntegrationError,
    not_found: &str,
    invalid: &str,
) -> CommandError {
    let category = error.category();
    match error {
        IntegrationError::NotFound => {
            tracing::debug!(category, "third-party resource not found");
            CommandError::UserInput(not_found.to_owned())
        }
        IntegrationError::InvalidInput { .. } => CommandError::UserInput(invalid.to_owned()),
        IntegrationError::NotConfigured { .. } => {
            CommandError::UserInput("该服务暂未配置或不可用。".to_owned())
        }
        IntegrationError::RateLimited { .. } => {
            tracing::warn!(category, "third-party service rate limited");
            CommandError::UserInput("请求过于频繁，请稍后再试。".to_owned())
        }
        IntegrationError::Unauthorized => {
            tracing::warn!(category, "third-party credentials rejected");
            CommandError::UserInput("第三方服务凭据无效或已过期。".to_owned())
        }
        IntegrationError::Timeout => {
            tracing::warn!(category, "third-party request timed out");
            CommandError::UserInput("查询超时，请稍后重试。".to_owned())
        }
        IntegrationError::ResponseTooLarge { .. } => {
            tracing::warn!(category, "third-party response too large");
            CommandError::UserInput("第三方响应内容过大，无法展示。".to_owned())
        }
        IntegrationError::Transport { .. }
        | IntegrationError::InvalidResponse { .. }
        | IntegrationError::Upstream { .. }
        | IntegrationError::AllProvidersFailed { .. }
        | IntegrationError::Unavailable { .. } => {
            tracing::warn!(category, "third-party request failed");
            CommandError::UserInput("第三方服务暂时不可用，请稍后重试。".to_owned())
        }
        _ => {
            tracing::warn!(category, "unclassified third-party failure");
            CommandError::UserInput("第三方服务暂时不可用，请稍后重试。".to_owned())
        }
    }
}
