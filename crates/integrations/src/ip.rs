use std::net::IpAddr;

use async_trait::async_trait;

use crate::error::IntegrationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IpProviderId {
    IpQuery,
    IpSb,
}

impl IpProviderId {
    /// Default attempt order, highest priority first.
    pub const ALL: [Self; 2] = [Self::IpQuery, Self::IpSb];

    pub const fn id(self) -> &'static str {
        match self {
            Self::IpQuery => "ipquery",
            Self::IpSb => "ipsb",
        }
    }

    pub const fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::IpQuery => &["ipquery.io", "ip-query"],
            Self::IpSb => &["ip.sb", "ip-sb"],
        }
    }

    pub const fn priority(self) -> u16 {
        match self {
            Self::IpQuery => 100,
            Self::IpSb => 80,
        }
    }

    /// Case-insensitive canonical id or alias lookup.
    pub fn parse(raw: &str) -> Option<Self> {
        let value = raw.trim().to_ascii_lowercase();
        if value.is_empty() {
            return None;
        }
        Self::ALL.into_iter().find(|provider| {
            value == provider.id() || provider.aliases().iter().any(|alias| value == *alias)
        })
    }
}

impl std::fmt::Display for IpProviderId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.id())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IpRisk {
    pub mobile: Option<bool>,
    pub vpn: Option<bool>,
    pub tor: Option<bool>,
    pub proxy: Option<bool>,
    pub datacenter: Option<bool>,
    pub score: Option<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IpData {
    pub ip: IpAddr,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub region: Option<String>,
    pub region_code: Option<String>,
    pub city: Option<String>,
    pub postal_code: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub timezone: Option<String>,
    pub asn: Option<String>,
    pub organization: Option<String>,
    pub isp: Option<String>,
    pub risk: Option<IpRisk>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IpQueryResult {
    pub provider: IpProviderId,
    pub data: IpData,
    pub failed_providers: Vec<IpProviderId>,
}

#[async_trait]
pub trait IpLookupService: Send + Sync {
    /// Queries the requested providers in order, or the default priority order when empty.
    async fn query(
        &self,
        ip: IpAddr,
        providers: &[IpProviderId],
    ) -> Result<IpQueryResult, IntegrationError>;
}

/// Expands and de-duplicates a user-supplied provider list.
///
/// Unknown names are returned separately so the command can list the available providers before
/// making any network request.
pub fn resolve_provider_selection(requested: &[String]) -> Result<Vec<IpProviderId>, Vec<String>> {
    if requested.is_empty() {
        return Ok(IpProviderId::ALL.to_vec());
    }

    let mut selected = Vec::new();
    let mut unknown = Vec::new();
    for raw in requested {
        if raw.trim().is_empty() {
            continue;
        }
        match IpProviderId::parse(raw) {
            Some(provider) if !selected.contains(&provider) => selected.push(provider),
            Some(_) => {}
            None => unknown.push(raw.clone()),
        }
    }

    if unknown.is_empty() {
        Ok(selected)
    } else {
        Err(unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ids_and_aliases_case_insensitively() {
        assert_eq!(IpProviderId::parse("IPQUERY"), Some(IpProviderId::IpQuery));
        assert_eq!(IpProviderId::parse("ip-query"), Some(IpProviderId::IpQuery));
        assert_eq!(IpProviderId::parse("ip.sb"), Some(IpProviderId::IpSb));
        assert_eq!(IpProviderId::parse("nope"), None);
    }

    #[test]
    fn default_order_follows_priority() {
        assert_eq!(
            IpProviderId::ALL,
            [IpProviderId::IpQuery, IpProviderId::IpSb]
        );
        assert!(IpProviderId::IpQuery.priority() > IpProviderId::IpSb.priority());
    }

    #[test]
    fn user_order_is_preserved_and_duplicates_removed() {
        let selected = resolve_provider_selection(&[
            "ipsb".to_owned(),
            "ipquery".to_owned(),
            "ip.sb".to_owned(),
        ])
        .unwrap();
        assert_eq!(selected, [IpProviderId::IpSb, IpProviderId::IpQuery]);
    }

    #[test]
    fn unknown_providers_are_reported() {
        let unknown =
            resolve_provider_selection(&["ipquery".to_owned(), "nope".to_owned()]).unwrap_err();
        assert_eq!(unknown, ["nope"]);
    }
}
