//! Bounded media loading built on the shared HTTP client and platform adapters.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::time::{Instant, timeout_at};
use url::Url;

use lukosbot_core::ports::{
    MediaLoadError, MediaLoadPolicy, MediaLoader, PlatformAdapter, PlatformError,
};
use lukosbot_domain::{MediaContent, MediaRef, MediaSource, PlatformKind};

use crate::http::{HttpClient, HttpError};

const MAX_REDIRECTS: usize = 5;

pub struct HttpMediaLoader {
    http: Arc<HttpClient>,
    adapters: HashMap<PlatformKind, Arc<dyn PlatformAdapter>>,
}

impl HttpMediaLoader {
    pub fn new(
        http: Arc<HttpClient>,
        adapters: HashMap<PlatformKind, Arc<dyn PlatformAdapter>>,
    ) -> Self {
        Self { http, adapters }
    }
}

#[async_trait]
impl MediaLoader for HttpMediaLoader {
    async fn load(
        &self,
        reference: &MediaRef,
        policy: MediaLoadPolicy,
    ) -> Result<MediaContent, MediaLoadError> {
        match &reference.source {
            MediaSource::Bytes(bytes) => {
                if bytes.len() as u64 > policy.max_bytes {
                    return Err(MediaLoadError::TooLarge {
                        limit: policy.max_bytes,
                    });
                }
                Ok(MediaContent {
                    bytes: bytes.clone(),
                    file_name: reference.file_name.clone(),
                    mime_type: reference.mime_type.clone(),
                })
            }
            MediaSource::PlatformFile { platform, file_id } => {
                let adapter =
                    self.adapters
                        .get(platform)
                        .ok_or(MediaLoadError::PlatformUnavailable {
                            platform: *platform,
                        })?;
                let content = adapter
                    .load_platform_file(file_id)
                    .await
                    .map_err(|source| match source {
                        PlatformError::Unsupported { .. } => MediaLoadError::UnsupportedSource {
                            part: "platform file download",
                        },
                        source => MediaLoadError::Platform { source },
                    })?;
                if content.bytes.len() as u64 > policy.max_bytes {
                    return Err(MediaLoadError::TooLarge {
                        limit: policy.max_bytes,
                    });
                }
                Ok(content)
            }
            MediaSource::Url(url) => self.load_url(reference, url, policy).await,
        }
    }
}

impl HttpMediaLoader {
    async fn load_url(
        &self,
        reference: &MediaRef,
        initial: &Url,
        policy: MediaLoadPolicy,
    ) -> Result<MediaContent, MediaLoadError> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut url = initial.clone();
        let mut redirects = 0;

        loop {
            validate_url(&url, policy.allowed_hosts, policy.https_only).await?;
            let response = timeout_at(
                deadline,
                self.http.execute(self.http.raw().get(url.clone())),
            )
            .await
            .map_err(|_| MediaLoadError::Timeout)?
            .map_err(map_http_error)?;

            if response.status().is_redirection() {
                if redirects >= MAX_REDIRECTS {
                    return Err(MediaLoadError::InvalidUrl);
                }
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or(MediaLoadError::InvalidUrl)?;
                url = response
                    .url()
                    .join(location)
                    .map_err(|_| MediaLoadError::InvalidUrl)?;
                redirects += 1;
                continue;
            }

            if !response.status().is_success() {
                return Err(MediaLoadError::Http {
                    source: Box::new(std::io::Error::other(format!(
                        "media request returned HTTP {}",
                        response.status()
                    ))),
                });
            }

            let mime_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
                .or_else(|| reference.mime_type.clone());
            let bytes = timeout_at(
                deadline,
                self.http
                    .read_limited_with_limit(response, policy.max_bytes),
            )
            .await
            .map_err(|_| MediaLoadError::Timeout)?
            .map_err(map_http_error)?;

            return Ok(MediaContent {
                bytes: bytes.into(),
                file_name: reference.file_name.clone(),
                mime_type,
            });
        }
    }
}

async fn validate_url(
    url: &Url,
    allowed_hosts: &'static [&'static str],
    https_only: bool,
) -> Result<(), MediaLoadError> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err(MediaLoadError::InvalidUrl);
    }
    if https_only && url.scheme() != "https" || !matches!(url.scheme(), "http" | "https") {
        return Err(MediaLoadError::InvalidUrl);
    }

    let host = url.host_str().ok_or(MediaLoadError::InvalidUrl)?;
    if !allowed_hosts
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(host))
    {
        return Err(MediaLoadError::RejectedHost);
    }

    let port = url
        .port_or_known_default()
        .ok_or(MediaLoadError::InvalidUrl)?;
    let addresses = timeout_at(
        Instant::now() + Duration::from_secs(5),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| MediaLoadError::Timeout)?
    .map_err(|error| MediaLoadError::Http {
        source: Box::new(error),
    })?;

    let mut resolved = false;
    for address in addresses {
        resolved = true;
        if !is_public_address(address.ip()) {
            return Err(MediaLoadError::RejectedHost);
        }
    }
    if !resolved {
        return Err(MediaLoadError::RejectedHost);
    }
    Ok(())
}

fn map_http_error(error: HttpError) -> MediaLoadError {
    match error {
        HttpError::Timeout => MediaLoadError::Timeout,
        HttpError::BodyTooLarge { limit } => MediaLoadError::TooLarge { limit },
        other => MediaLoadError::Http {
            source: Box::new(other),
        },
    }
}

fn is_public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1])))
        }
        IpAddr::V6(ip) => {
            if let Some(ip) = ip.to_ipv4_mapped() {
                return is_public_address(IpAddr::V4(ip));
            }
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unique_local()
                || ip.is_unicast_link_local())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_private_and_mapped_addresses() {
        assert!(!is_public_address("127.0.0.1".parse().unwrap()));
        assert!(!is_public_address("10.0.0.1".parse().unwrap()));
        assert!(!is_public_address("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!is_public_address("::ffff:10.0.0.1".parse().unwrap()));
        assert!(is_public_address("1.1.1.1".parse().unwrap()));
        assert!(is_public_address("2606:4700:4700::1111".parse().unwrap()));
    }

    #[tokio::test]
    async fn rejects_urls_outside_the_allowlist() {
        let url = Url::parse("https://evil.example/a.png").unwrap();
        let error = validate_url(&url, &["cdn.example"], true)
            .await
            .unwrap_err();
        assert!(matches!(error, MediaLoadError::RejectedHost));
    }

    #[tokio::test]
    async fn rejects_http_when_https_is_required() {
        let url = Url::parse("http://cdn.example/a.png").unwrap();
        let error = validate_url(&url, &["cdn.example"], true)
            .await
            .unwrap_err();
        assert!(matches!(error, MediaLoadError::InvalidUrl));
    }
}
