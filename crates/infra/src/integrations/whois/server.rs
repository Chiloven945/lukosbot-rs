use std::net::IpAddr;

pub(crate) const DEFAULT_SERVER: &str = "whois.internic.net";
pub(crate) const IANA_SERVER: &str = "whois.iana.org";
pub(crate) const CNNIC_SERVER: &str = "whois.cnnic.cn";

/// A validated WHOIS server hostname.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WhoisServer(String);

impl WhoisServer {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }

    pub(crate) fn parse(raw: &str) -> Option<Self> {
        validate_hostname(raw).map(Self)
    }

    fn from_static(value: &'static str) -> Self {
        Self(value.to_owned())
    }
}

pub(crate) fn default_server() -> WhoisServer {
    WhoisServer::from_static(DEFAULT_SERVER)
}

pub(crate) fn iana_server() -> WhoisServer {
    WhoisServer::from_static(IANA_SERVER)
}

/// Selects a server from the JVM-compatible static registry.
pub(crate) fn static_server(tld: &str) -> Option<WhoisServer> {
    match tld {
        "com" | "net" | "edu" => Some(default_server()),
        "cn" | "xn--fiqs8s" => Some(WhoisServer::from_static(CNNIC_SERVER)),
        _ => None,
    }
}

/// Extracts the first valid `whois:` referral from an IANA response.
pub(crate) fn parse_iana_referral(text: &str) -> Option<WhoisServer> {
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("whois")
            && let Some(server) = WhoisServer::parse(value)
        {
            return Some(server);
        }
    }
    None
}

fn validate_hostname(raw: &str) -> Option<String> {
    let host = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.len() > 253 || !host.is_ascii() || host.parse::<IpAddr>().is_ok() {
        return None;
    }

    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return None;
    }

    if labels.iter().all(|label| {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_static_tlds() {
        assert_eq!(static_server("com").unwrap().as_str(), "whois.internic.net");
        assert_eq!(static_server("cn").unwrap().as_str(), "whois.cnnic.cn");
        assert_eq!(
            static_server("xn--fiqs8s").unwrap().as_str(),
            "whois.cnnic.cn"
        );
        assert!(static_server("io").is_none());
    }

    #[test]
    fn parses_iana_referrals_with_whitespace() {
        let text = "% comment\ndomain: IO\nwhois:          whois.nic.io\n";
        assert_eq!(parse_iana_referral(text).unwrap().as_str(), "whois.nic.io");
    }

    #[test]
    fn rejects_invalid_referral_hosts() {
        assert!(parse_iana_referral("whois: localhost\n").is_none());
        assert!(parse_iana_referral("whois: 127.0.0.1\n").is_none());
        assert!(parse_iana_referral("whois: bad host\n").is_none());
        assert!(parse_iana_referral("whois: -bad.example\n").is_none());
    }

    #[test]
    fn rejects_invalid_server_names() {
        assert!(WhoisServer::parse("localhost").is_none());
        assert!(WhoisServer::parse("whois.example.").is_some());
        assert!(WhoisServer::parse("whois.example:43").is_none());
    }

    #[test]
    fn default_and_iana_constants_are_present() {
        assert_eq!(default_server().as_str(), DEFAULT_SERVER);
        assert_eq!(iana_server().as_str(), IANA_SERVER);
    }
}
