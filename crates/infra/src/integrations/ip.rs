use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use lukosbot_integrations::{
    BoxError, IntegrationError, IpData, IpLookupService, IpProviderId, IpQueryResult, IpRisk,
};

use crate::http::{HttpClient, HttpError};

const PROVIDER_BUDGET: Duration = Duration::from_secs(3);
const TOTAL_BUDGET: Duration = Duration::from_secs(6);
const IPQUERY_BASE: &str = "https://api.ipquery.io/";
const IPSB_BASE: &str = "https://api.ip.sb/";

#[derive(Debug, Clone)]
struct IpEndpoints {
    ipquery: Url,
    ipsb: Url,
}

impl Default for IpEndpoints {
    fn default() -> Self {
        Self {
            ipquery: Url::parse(IPQUERY_BASE).expect("the built-in ipquery URL is valid"),
            ipsb: Url::parse(IPSB_BASE).expect("the built-in ipsb URL is valid"),
        }
    }
}

pub struct IpLookupClient {
    http: Arc<HttpClient>,
    endpoints: IpEndpoints,
}

impl IpLookupClient {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self {
            http,
            endpoints: IpEndpoints::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_endpoints(http: Arc<HttpClient>, ipquery: Url, ipsb: Url) -> Self {
        Self {
            http,
            endpoints: IpEndpoints { ipquery, ipsb },
        }
    }

    async fn query_provider(
        &self,
        provider: IpProviderId,
        ip: IpAddr,
    ) -> Result<IpData, IntegrationError> {
        tokio::time::timeout(PROVIDER_BUDGET, self.query_provider_inner(provider, ip))
            .await
            .map_err(|_| IntegrationError::Timeout)?
    }

    async fn query_provider_inner(
        &self,
        provider: IpProviderId,
        ip: IpAddr,
    ) -> Result<IpData, IntegrationError> {
        match provider {
            IpProviderId::IpQuery => {
                let url = provider_url(self.endpoints.ipquery.clone(), &[&ip.to_string()])?;
                let dto: IpQueryDto = self.http.get_json(url).await.map_err(map_http_error)?;
                Ok(dto.into_data(ip))
            }
            IpProviderId::IpSb => {
                let url = provider_url(self.endpoints.ipsb.clone(), &["geoip", &ip.to_string()])?;
                let dto: IpSbDto = self.http.get_json(url).await.map_err(map_http_error)?;
                Ok(dto.into_data(ip))
            }
        }
    }
}

#[async_trait]
impl IpLookupService for IpLookupClient {
    async fn query(
        &self,
        ip: IpAddr,
        providers: &[IpProviderId],
    ) -> Result<IpQueryResult, IntegrationError> {
        let targets: Vec<IpProviderId> = if providers.is_empty() {
            IpProviderId::ALL.to_vec()
        } else {
            providers.to_vec()
        };

        let deadline = tokio::time::Instant::now() + TOTAL_BUDGET;
        let mut failed = Vec::new();
        for provider in &targets {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                failed.push(*provider);
                continue;
            }
            let result = tokio::time::timeout(remaining, self.query_provider(*provider, ip)).await;
            match result {
                Ok(Ok(data)) => {
                    return Ok(IpQueryResult {
                        provider: *provider,
                        data,
                        failed_providers: failed,
                    });
                }
                Ok(Err(error)) => {
                    tracing::warn!(
                        provider = provider.id(),
                        category = error.category(),
                        "IP provider failed; trying the next provider"
                    );
                    failed.push(*provider);
                }
                Err(_) => {
                    tracing::warn!(provider = provider.id(), "IP provider timed out");
                    failed.push(*provider);
                }
            }
        }

        Err(IntegrationError::AllProvidersFailed { providers: targets })
    }
}

fn provider_url(mut base: Url, segments: &[&str]) -> Result<Url, IntegrationError> {
    base.path_segments_mut()
        .map_err(|_| IntegrationError::InvalidInput {
            message: "IP provider base URL is not hierarchical".to_owned(),
        })?
        .extend(segments);
    Ok(base)
}

fn map_http_error(error: HttpError) -> IntegrationError {
    match error {
        HttpError::Timeout => IntegrationError::Timeout,
        HttpError::BodyTooLarge { limit } => IntegrationError::ResponseTooLarge { limit },
        HttpError::Json(error) => IntegrationError::InvalidResponse {
            source: Box::new(error),
        },
        HttpError::Status { status, .. } => match status.as_u16() {
            404 => IntegrationError::NotFound,
            401 | 403 => IntegrationError::Unauthorized,
            429 => IntegrationError::RateLimited { retry_after: None },
            status => IntegrationError::Upstream { status },
        },
        other => IntegrationError::Transport {
            source: Box::new(other) as BoxError,
        },
    }
}

#[derive(Debug, Deserialize)]
struct IpQueryDto {
    #[serde(default)]
    ip: Option<String>,
    #[serde(default)]
    isp: Option<IpQueryIspDto>,
    #[serde(default)]
    location: Option<IpQueryLocationDto>,
    #[serde(default)]
    risk: Option<IpQueryRiskDto>,
}

#[derive(Debug, Deserialize)]
struct IpQueryIspDto {
    #[serde(default)]
    asn: Option<String>,
    #[serde(default)]
    org: Option<String>,
    #[serde(default)]
    isp: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IpQueryLocationDto {
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    country_code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    zipcode: Option<String>,
    #[serde(default)]
    latitude: Option<f64>,
    #[serde(default)]
    longitude: Option<f64>,
    #[serde(default)]
    timezone: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IpQueryRiskDto {
    #[serde(default, rename = "is_mobile")]
    mobile: Option<bool>,
    #[serde(default, rename = "is_vpn")]
    vpn: Option<bool>,
    #[serde(default, rename = "is_tor")]
    tor: Option<bool>,
    #[serde(default, rename = "is_proxy")]
    proxy: Option<bool>,
    #[serde(default, rename = "is_datacenter")]
    datacenter: Option<bool>,
    #[serde(default)]
    risk_score: Option<u8>,
}

impl IpQueryDto {
    fn into_data(self, requested: IpAddr) -> IpData {
        let ip = self
            .ip
            .as_deref()
            .and_then(|value| value.parse().ok())
            .unwrap_or(requested);
        let location = self.location.unwrap_or(IpQueryLocationDto {
            country: None,
            country_code: None,
            state: None,
            city: None,
            zipcode: None,
            latitude: None,
            longitude: None,
            timezone: None,
        });
        let isp = self.isp.unwrap_or(IpQueryIspDto {
            asn: None,
            org: None,
            isp: None,
        });

        IpData {
            ip,
            country: location.country,
            country_code: location.country_code,
            region: location.state,
            region_code: None,
            city: location.city,
            postal_code: location.zipcode,
            latitude: location.latitude,
            longitude: location.longitude,
            timezone: location.timezone,
            asn: isp.asn,
            organization: isp.org,
            isp: isp.isp,
            risk: self.risk.map(IpQueryRiskDto::into_risk),
        }
    }
}

impl IpQueryRiskDto {
    fn into_risk(self) -> IpRisk {
        IpRisk {
            mobile: self.mobile,
            vpn: self.vpn,
            tor: self.tor,
            proxy: self.proxy,
            datacenter: self.datacenter,
            score: self.risk_score,
        }
    }
}

#[derive(Debug, Deserialize)]
struct IpSbDto {
    #[serde(default)]
    ip: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    country_code: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    region_code: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    postal_code: Option<String>,
    #[serde(default)]
    latitude: Option<f64>,
    #[serde(default)]
    longitude: Option<f64>,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    asn: Option<String>,
    #[serde(default)]
    organization: Option<String>,
}

impl IpSbDto {
    fn into_data(self, requested: IpAddr) -> IpData {
        let ip = self
            .ip
            .as_deref()
            .and_then(|value| value.parse().ok())
            .unwrap_or(requested);
        IpData {
            ip,
            country: self.country,
            country_code: self.country_code,
            region: self.region,
            region_code: self.region_code,
            city: self.city,
            postal_code: self.postal_code,
            latitude: self.latitude,
            longitude: self.longitude,
            timezone: self.timezone,
            asn: self.asn,
            organization: self.organization,
            isp: None,
            risk: None,
        }
    }
}

#[cfg(test)]
mod tests;
