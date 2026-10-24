use async_trait::async_trait;
use bytes::Bytes;
use url::{Host, Url};

use crate::IntegrationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WikiSite {
    Wikipedia,
    MinecraftWiki,
}

impl WikiSite {
    pub const fn domain_root(self) -> &'static str {
        match self {
            Self::Wikipedia => "wikipedia.org",
            Self::MinecraftWiki => "minecraft.wiki",
        }
    }

    pub const fn path_prefix(self) -> &'static str {
        match self {
            Self::Wikipedia => "/wiki/",
            Self::MinecraftWiki => "/w/",
        }
    }

    pub const fn default_language(self) -> &'static str {
        "zh"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiTarget {
    pub site: WikiSite,
    pub url: Url,
    pub language: String,
    pub title: Option<String>,
}

impl WikiTarget {
    pub fn parse(site: WikiSite, input: &str) -> Result<Self, IntegrationError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(invalid("wiki input must not be empty"));
        }

        if input.starts_with("http://") || input.starts_with("https://") {
            let url = Url::parse(input).map_err(|_| invalid("invalid wiki URL"))?;
            validate_url(site, &url)?;
            return Ok(Self {
                site,
                language: url
                    .host_str()
                    .and_then(|host| host.split('.').next())
                    .unwrap_or(site.default_language())
                    .to_owned(),
                url,
                title: None,
            });
        }

        let (language, title) = split_language(input, site.default_language());
        let title = title.trim().replace(' ', "_");
        if title.is_empty() {
            return Err(invalid("wiki title must not be empty"));
        }

        let mut url = Url::parse(&format!(
            "https://{}.{}{}",
            language,
            site.domain_root(),
            site.path_prefix().trim_end_matches('/')
        ))
        .map_err(|_| invalid("invalid wiki target"))?;
        url.path_segments_mut()
            .map_err(|_| invalid("invalid wiki URL"))?
            .push(&title);

        Ok(Self {
            site,
            url,
            language,
            title: Some(title),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiSummary {
    pub title: String,
    pub lead: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedDocument {
    pub bytes: Bytes,
    pub file_name: String,
    pub mime_type: String,
}

#[async_trait]
pub trait WikiService: Send + Sync {
    async fn summary(&self, target: &WikiTarget) -> Result<WikiSummary, IntegrationError>;

    async fn export_markdown(
        &self,
        target: &WikiTarget,
    ) -> Result<RenderedDocument, IntegrationError>;

    async fn screenshot_intro(
        &self,
        target: &WikiTarget,
    ) -> Result<RenderedDocument, IntegrationError>;
}

fn validate_url(site: WikiSite, url: &Url) -> Result<(), IntegrationError> {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(invalid(
            "wiki URL must be HTTPS without credentials or a custom port",
        ));
    }

    let Some(Host::Domain(host)) = url.host() else {
        return Err(invalid("wiki URL must use a domain name"));
    };
    let host = host.to_ascii_lowercase();
    let root = site.domain_root();
    let valid = if host == root {
        true
    } else if let Some(label) = host.strip_suffix(&format!(".{root}")) {
        is_language(label)
    } else {
        false
    };
    if !valid {
        return Err(invalid("wiki URL host is not allowed"));
    }

    if !url.path().starts_with(site.path_prefix()) {
        return Err(invalid("wiki URL path is not an article path"));
    }
    Ok(())
}

fn split_language(input: &str, fallback: &str) -> (String, String) {
    let Some(index) = input.find(':') else {
        return (fallback.to_owned(), input.to_owned());
    };
    if !(1..=6).contains(&index) {
        return (fallback.to_owned(), input.to_owned());
    }
    let candidate = &input[..index];
    if !is_language(candidate) {
        return (fallback.to_owned(), input.to_owned());
    }
    (
        candidate.to_ascii_lowercase(),
        input[index + 1..].to_owned(),
    )
}

fn is_language(value: &str) -> bool {
    value != "www"
        && (2..=6).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || byte == b'-')
}

fn invalid(message: &str) -> IntegrationError {
    IntegrationError::InvalidInput {
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_article_urls_from_titles_and_languages() {
        let target = WikiTarget::parse(WikiSite::Wikipedia, "en:Albert Einstein").unwrap();
        assert_eq!(target.language, "en");
        assert_eq!(
            target.url.as_str(),
            "https://en.wikipedia.org/wiki/Albert_Einstein"
        );

        let target = WikiTarget::parse(WikiSite::MinecraftWiki, "僵尸猪灵").unwrap();
        assert_eq!(
            target.url.as_str(),
            "https://zh.minecraft.wiki/w/%E5%83%B5%E5%B0%B8%E7%8C%AA%E7%81%B5"
        );
    }

    #[test]
    fn accepts_only_matching_https_hosts() {
        assert!(
            WikiTarget::parse(WikiSite::Wikipedia, "https://en.wikipedia.org/wiki/Java").is_ok()
        );
        for invalid in [
            "http://en.wikipedia.org/wiki/Java",
            "https://wikipedia.org.evil.example/wiki/Java",
            "https://www.wikipedia.org/wiki/Java",
            "https://en.minecraft.wiki/w/Java",
            "https://en.wikipedia.org:8443/wiki/Java",
        ] {
            assert!(WikiTarget::parse(WikiSite::Wikipedia, invalid).is_err());
        }
    }

    #[test]
    fn treats_non_language_colons_as_title_text() {
        let target = WikiTarget::parse(WikiSite::Wikipedia, "a:Title").unwrap();
        assert_eq!(target.title.as_deref(), Some("a:Title"));
    }
}
