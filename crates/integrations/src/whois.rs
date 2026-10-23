use async_trait::async_trait;

use crate::error::IntegrationError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhoisRecord {
    pub domain: String,
    pub server: String,
    /// Cleaned WHOIS text. The service has already enforced the final display limit.
    pub text: String,
    pub truncated: bool,
}

#[async_trait]
pub trait WhoisService: Send + Sync {
    async fn query(&self, domain: &str) -> Result<WhoisRecord, IntegrationError>;
}
