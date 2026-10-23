use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, lookup_host};
use tokio::time::Instant;

use lukosbot_integrations::{BoxError, IntegrationError, WhoisRecord, WhoisService};

const DEFAULT_SERVER: &str = "whois.internic.net";
const IANA_SERVER: &str = "whois.iana.org";
const CNNIC_SERVER: &str = "whois.cnnic.cn";
const IANA_MAX_BYTES: usize = 16 * 1024;
const MAX_DISPLAY_CHARS: usize = 1_800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhoisSettings {
    pub connect_timeout: Duration,
    pub total_timeout: Duration,
    pub max_response_bytes: u64,
}

impl Default for WhoisSettings {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(3),
            total_timeout: Duration::from_secs(8),
            max_response_bytes: 64 * 1024,
        }
    }
}

pub struct WhoisClient {
    settings: WhoisSettings,
}

impl WhoisClient {
    pub fn new(settings: WhoisSettings) -> Self {
        Self { settings }
    }

    async fn query_server(
        &self,
        server: &str,
        domain: &str,
        deadline: Instant,
    ) -> Result<(String, bool), IntegrationError> {
        let host = validate_hostname(server).ok_or_else(|| IntegrationError::InvalidInput {
            message: "the WHOIS server hostname is invalid".to_owned(),
        })?;
        let address = resolve_public(&host, deadline).await?;
        exchange_addr(
            address,
            domain,
            deadline,
            self.settings.connect_timeout,
            self.settings.max_response_bytes as usize,
        )
        .await
    }

    async fn lookup_iana(&self, tld: &str, deadline: Instant) -> Option<String> {
        let host = IANA_SERVER.to_owned();
        let address = resolve_public(&host, deadline).await.ok()?;
        let (text, _) = exchange_addr(
            address,
            tld,
            deadline,
            self.settings.connect_timeout,
            IANA_MAX_BYTES,
        )
        .await
        .ok()?;
        parse_iana_referral(&text)
    }
}

#[async_trait]
impl WhoisService for WhoisClient {
    async fn query(&self, domain: &str) -> Result<WhoisRecord, IntegrationError> {
        let domain = normalize_domain(domain)?;
        let deadline = Instant::now() + self.settings.total_timeout;
        let tld = domain
            .rsplit_once('.')
            .map(|(_, tld)| tld)
            .unwrap_or(domain.as_str());

        let server = match static_server(tld) {
            Some(server) => server.to_owned(),
            None => self
                .lookup_iana(tld, deadline)
                .await
                .unwrap_or_else(|| DEFAULT_SERVER.to_owned()),
        };

        let (raw, raw_truncated) = self.query_server(&server, &domain, deadline).await?;
        let (text, display_truncated) = clean_whois(&raw);
        Ok(WhoisRecord {
            domain,
            server,
            text,
            truncated: raw_truncated || display_truncated,
        })
    }
}

fn static_server(tld: &str) -> Option<&'static str> {
    match tld {
        "com" | "net" | "edu" => Some(DEFAULT_SERVER),
        "cn" | "xn--fiqs8s" => Some(CNNIC_SERVER),
        _ => None,
    }
}

/// Normalises a domain and rejects values that could inject a second protocol line.
pub fn normalize_domain(raw: &str) -> Result<String, IntegrationError> {
    if raw.chars().any(char::is_control) {
        return Err(invalid_domain());
    }
    let value = raw.trim().trim_end_matches('.');
    if value.is_empty()
        || value.len() > 253
        || value.chars().any(|character| {
            character.is_whitespace() || matches!(character, '/' | ':' | '?' | '#')
        })
        || value.parse::<IpAddr>().is_ok()
    {
        return Err(invalid_domain());
    }

    let ascii = idna::domain_to_ascii(value)
        .map_err(|_| invalid_domain())?
        .to_ascii_lowercase();
    if ascii.is_empty() || ascii.len() > 253 {
        return Err(invalid_domain());
    }

    for label in ascii.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(invalid_domain());
        }
    }

    Ok(ascii)
}

fn invalid_domain() -> IntegrationError {
    IntegrationError::InvalidInput {
        message: "the domain is invalid".to_owned(),
    }
}

fn validate_hostname(raw: &str) -> Option<String> {
    let host = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.len() > 253 || !host.is_ascii() || host.parse::<IpAddr>().is_ok() {
        return None;
    }
    if host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    }) {
        Some(host)
    } else {
        None
    }
}

fn parse_iana_referral(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("whois:")
            && let Some(host) = validate_hostname(value)
        {
            return Some(host);
        }
    }
    None
}

async fn resolve_public(host: &str, deadline: Instant) -> Result<SocketAddr, IntegrationError> {
    let addrs = tokio::time::timeout_at(deadline, lookup_host((host, 43)))
        .await
        .map_err(|_| IntegrationError::Timeout)?
        .map_err(|error| IntegrationError::Transport {
            source: Box::new(error) as BoxError,
        })?;

    for address in addrs {
        if is_public(address.ip()) {
            return Ok(address);
        }
    }
    Err(IntegrationError::Transport {
        source: Box::new(std::io::Error::other(
            "the WHOIS hostname has no public address",
        )) as BoxError,
    })
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast())
        }
        IpAddr::V6(ip) => {
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unique_local()
                || ip.is_unicast_link_local())
        }
    }
}

async fn exchange_addr(
    address: SocketAddr,
    query: &str,
    deadline: Instant,
    connect_timeout: Duration,
    max_bytes: usize,
) -> Result<(String, bool), IntegrationError> {
    let connect_deadline = deadline.min(Instant::now() + connect_timeout);
    let mut stream = tokio::time::timeout_at(connect_deadline, TcpStream::connect(address))
        .await
        .map_err(|_| IntegrationError::Timeout)?
        .map_err(|error| IntegrationError::Transport {
            source: Box::new(error) as BoxError,
        })?;

    let line = format!("{query}\r\n");
    tokio::time::timeout_at(deadline, stream.write_all(line.as_bytes()))
        .await
        .map_err(|_| IntegrationError::Timeout)?
        .map_err(|error| IntegrationError::Transport {
            source: Box::new(error) as BoxError,
        })?;

    let mut body = Vec::new();
    let mut buffer = [0_u8; 4096];
    let mut truncated = false;
    loop {
        let read = match tokio::time::timeout_at(deadline, stream.read(&mut buffer)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(read)) => read,
            Ok(Err(error)) => {
                return Err(IntegrationError::Transport {
                    source: Box::new(error) as BoxError,
                });
            }
            Err(_) => return Err(IntegrationError::Timeout),
        };
        let remaining = max_bytes.saturating_sub(body.len());
        if read > remaining {
            body.extend_from_slice(&buffer[..remaining]);
            truncated = true;
            break;
        }
        body.extend_from_slice(&buffer[..read]);
    }

    Ok((String::from_utf8_lossy(&body).into_owned(), truncated))
}

/// Applies the JVM-compatible WHOIS cleanup rules and a final, character-based display limit.
pub fn clean_whois(raw: &str) -> (String, bool) {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    for line in normalized.lines() {
        if is_boilerplate(line) {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.ends_with(':')
            && trimmed
                .split_once(':')
                .is_some_and(|(_, value)| value.trim().is_empty())
        {
            continue;
        }
        lines.push(trimmed.to_owned());
    }

    let mut merged = Vec::new();
    let mut key_index: HashMap<String, usize> = HashMap::new();
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            merged.push(line);
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            merged.push(line);
            continue;
        }
        if let Some(index) = key_index.get(key) {
            let target: &mut String = &mut merged[*index];
            target.push_str(", ");
            target.push_str(value);
        } else {
            key_index.insert(key.to_owned(), merged.len());
            merged.push(format!("{key}: {value}"));
        }
    }

    let mut compact = Vec::new();
    let mut blank_run = 0;
    for line in merged {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 2 {
                compact.push(String::new());
            }
        } else {
            blank_run = 0;
            compact.push(filter_controls(&line));
        }
    }

    let text = compact.join("\n").trim().to_owned();
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > MAX_DISPLAY_CHARS {
        (chars[..MAX_DISPLAY_CHARS].iter().collect(), true)
    } else {
        (text, false)
    }
}

fn is_boilerplate(line: &str) -> bool {
    let line = line.trim_start();
    let lower = line.to_ascii_lowercase();
    lower.starts_with("for more information on whois status codes")
        || lower.starts_with("notice:")
        || lower.starts_with("terms of use:")
        || lower.starts_with("whois terms of use")
        || lower.starts_with(">>> last update of whois database:")
}

fn filter_controls(line: &str) -> String {
    line.chars()
        .filter(|character| !character.is_control() || *character == '\t')
        .collect()
}

#[cfg(test)]
mod tests;
