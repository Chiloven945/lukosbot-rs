use async_trait::async_trait;
use tokio::time::Instant;

use lukosbot_integrations::{IntegrationError, WhoisRecord, WhoisService};

use crate::integrations::whois::clean::{clean_whois, is_not_found};
use crate::integrations::whois::domain::normalize_domain;
use crate::integrations::whois::server::{
    default_server, iana_server, parse_iana_referral, static_server,
};
use crate::integrations::whois::transport::query_server;

#[cfg(test)]
use std::net::SocketAddr;

#[cfg(test)]
use crate::integrations::whois::transport::query_server_at;

mod clean;
mod domain;
mod server;
mod transport;

pub use transport::WhoisSettings;

#[cfg(test)]
mod tests;

const IANA_MAX_BYTES: usize = 16 * 1024;

enum IanaLookup {
    Referral(server::WhoisServer),
    NoReferral,
    Failed,
}

pub struct WhoisClient {
    settings: WhoisSettings,
    #[cfg(test)]
    test_endpoints: Option<TestEndpoints>,
}

#[cfg(test)]
struct TestEndpoints {
    iana: SocketAddr,
    registry: SocketAddr,
}

impl WhoisClient {
    pub fn new(settings: WhoisSettings) -> Self {
        Self {
            settings,
            #[cfg(test)]
            test_endpoints: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_endpoints(
        settings: WhoisSettings,
        iana: SocketAddr,
        registry: SocketAddr,
    ) -> Self {
        Self {
            settings,
            test_endpoints: Some(TestEndpoints { iana, registry }),
        }
    }

    async fn lookup_iana(&self, tld: &str, deadline: Instant) -> IanaLookup {
        #[cfg(test)]
        if let Some(endpoints) = &self.test_endpoints {
            let result = query_server_at(
                endpoints.iana,
                tld,
                deadline,
                self.settings.connect_timeout,
                IANA_MAX_BYTES,
            )
            .await;
            return match result {
                Ok((text, _)) => match parse_iana_referral(&text) {
                    Some(server) => IanaLookup::Referral(server),
                    None => IanaLookup::NoReferral,
                },
                Err(_) => IanaLookup::Failed,
            };
        }

        match query_server(
            iana_server().as_str(),
            tld,
            deadline,
            self.settings.connect_timeout,
            IANA_MAX_BYTES,
        )
        .await
        {
            Ok((text, _)) => match parse_iana_referral(&text) {
                Some(server) => IanaLookup::Referral(server),
                None => IanaLookup::NoReferral,
            },
            Err(_) => IanaLookup::Failed,
        }
    }

    async fn query_registry(
        &self,
        server: &server::WhoisServer,
        domain: &str,
        deadline: Instant,
    ) -> Result<(String, bool), IntegrationError> {
        #[cfg(test)]
        if let Some(endpoints) = &self.test_endpoints {
            return query_server_at(
                endpoints.registry,
                domain,
                deadline,
                self.settings.connect_timeout,
                self.settings.max_response_bytes as usize,
            )
            .await;
        }

        query_server(
            server.as_str(),
            domain,
            deadline,
            self.settings.connect_timeout,
            self.settings.max_response_bytes as usize,
        )
        .await
    }
}

#[async_trait]
impl WhoisService for WhoisClient {
    async fn query(&self, domain: &str) -> Result<WhoisRecord, IntegrationError> {
        let domain = normalize_domain(domain)?;
        let deadline = Instant::now() + self.settings.total_timeout;

        let server = match static_server(domain.tld()) {
            Some(server) => server,
            None => match self.lookup_iana(domain.tld(), deadline).await {
                IanaLookup::Referral(server) => server,
                IanaLookup::NoReferral => return Err(IntegrationError::NotFound),
                IanaLookup::Failed => default_server(),
            },
        };

        let (raw, raw_truncated) = self
            .query_registry(&server, domain.as_str(), deadline)
            .await?;
        let cleaned = clean_whois(&raw);
        if cleaned.text.is_empty() || is_not_found(&cleaned.text) {
            return Err(IntegrationError::NotFound);
        }

        Ok(WhoisRecord {
            domain: domain.into_string(),
            server: server.into_string(),
            text: cleaned.text,
            truncated: raw_truncated || cleaned.truncated,
        })
    }
}
