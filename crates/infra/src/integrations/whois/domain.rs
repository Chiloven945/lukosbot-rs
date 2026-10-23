use std::net::IpAddr;

use lukosbot_integrations::IntegrationError;

/// A domain that has passed syntax and IDNA validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedDomain(String);

impl NormalizedDomain {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }

    pub(crate) fn tld(&self) -> &str {
        self.0
            .rsplit_once('.')
            .map(|(_, tld)| tld)
            .unwrap_or(self.0.as_str())
    }
}

/// Normalizes a domain and rejects values that could inject a second protocol line.
pub(crate) fn normalize_domain(raw: &str) -> Result<NormalizedDomain, IntegrationError> {
    if raw.chars().any(char::is_control) {
        return Err(invalid_domain());
    }

    let trimmed = raw.trim();
    let body = trimmed.strip_suffix('.').unwrap_or(trimmed);
    if body.is_empty()
        || body.ends_with('.')
        || body.starts_with('.')
        || !body.contains('.')
        || body.chars().any(|character| {
            character.is_whitespace() || matches!(character, '/' | ':' | '?' | '#' | '@')
        })
        || body.parse::<IpAddr>().is_ok()
    {
        return Err(invalid_domain());
    }

    let ascii = idna::domain_to_ascii(body)
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

    Ok(NormalizedDomain(ascii))
}

fn invalid_domain() -> IntegrationError {
    IntegrationError::InvalidInput {
        message: "the domain is invalid".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_ascii_and_idna_domains() {
        assert_eq!(
            normalize_domain("Example.COM.").unwrap().as_str(),
            "example.com"
        );
        assert_eq!(
            normalize_domain("例子.中国").unwrap().as_str(),
            "xn--fsqu00a.xn--fiqs8s"
        );
    }

    #[test]
    fn requires_a_tld_and_one_trailing_dot_at_most() {
        for invalid in ["localhost", "com", "example.com..", ".example.com"] {
            assert!(normalize_domain(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn rejects_injection_ip_ports_and_bad_labels() {
        for invalid in [
            "",
            "example.com\r\n",
            "example.com:43",
            "https://example.com",
            "1.1.1.1",
            "-bad.example",
            "bad-.example",
            "a..b",
            "bad label.example",
            "bad@example.com",
        ] {
            assert!(normalize_domain(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn exposes_the_tld() {
        assert_eq!(normalize_domain("www.example.com").unwrap().tld(), "com");
        assert_eq!(normalize_domain("例子.中国").unwrap().tld(), "xn--fiqs8s");
    }
}
