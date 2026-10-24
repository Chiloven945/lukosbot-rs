use std::sync::Arc;
use tokio::time::Instant;

use async_trait::async_trait;
use bytes::Bytes;
use scraper::{Html, Selector};
use tokio::time::timeout_at;
use url::Url;

use lukosbot_integrations::{
    IntegrationError, RenderedDocument, WikiService, WikiSite, WikiSummary, WikiTarget,
};

use crate::http::{HttpClient, HttpError};
use crate::web::WebSettings;
use crate::web::screenshot::ScreenshotHandle;

pub struct WikiClient {
    http: Arc<HttpClient>,
    settings: WebSettings,
    screenshot: Option<ScreenshotHandle>,
}

impl WikiClient {
    pub fn new(http: Arc<HttpClient>, settings: WebSettings) -> Self {
        Self {
            http,
            settings,
            screenshot: None,
        }
    }

    pub fn with_screenshot(mut self, screenshot: ScreenshotHandle) -> Self {
        self.screenshot = Some(screenshot);
        self
    }

    async fn fetch_html(&self, target: &WikiTarget) -> Result<String, IntegrationError> {
        let deadline = Instant::now() + self.settings.request_timeout;
        let mut url = target.url.clone();

        for _ in 0..=self.settings.max_redirects {
            validate_target(target.site, &url)?;
            let response = timeout_at(
                deadline,
                self.http.execute(
                    self.http
                        .raw()
                        .get(url.clone())
                        .header(reqwest::header::USER_AGENT, &self.settings.user_agent),
                ),
            )
            .await
            .map_err(|_| IntegrationError::Timeout)?
            .map_err(map_http)?;

            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| invalid("wiki redirect has no Location"))?;
                url = response
                    .url()
                    .join(location)
                    .map_err(|_| invalid("invalid wiki redirect"))?;
                validate_target(target.site, &url)?;
                continue;
            }

            if !response.status().is_success() {
                return Err(IntegrationError::Upstream {
                    status: response.status().as_u16(),
                });
            }

            let bytes = timeout_at(
                deadline,
                self.http
                    .read_limited_with_limit(response, self.settings.max_html_bytes),
            )
            .await
            .map_err(|_| IntegrationError::Timeout)?
            .map_err(map_http)?;
            return String::from_utf8(bytes).map_err(|error| IntegrationError::InvalidResponse {
                source: Box::new(error),
            });
        }

        Err(invalid("wiki redirect limit exceeded"))
    }
}

#[async_trait]
impl WikiService for WikiClient {
    async fn summary(&self, target: &WikiTarget) -> Result<WikiSummary, IntegrationError> {
        let html = self.fetch_html(target).await?;
        Ok(summary_from_html(&html))
    }

    async fn export_markdown(
        &self,
        target: &WikiTarget,
    ) -> Result<RenderedDocument, IntegrationError> {
        let html = self.fetch_html(target).await?;
        let document = Html::parse_document(&html);
        let title = select_text(&document, "h1#firstHeading").unwrap_or_default();
        let selector = match target.site {
            WikiSite::Wikipedia => "#content",
            WikiSite::MinecraftWiki => "#mw-content-text",
        };
        let content = select_first(&document, selector)
            .map(|element| element.html())
            .unwrap_or_else(|| document.root_element().html());

        let markdown =
            htmd::convert(&content).map_err(|error| IntegrationError::InvalidResponse {
                source: Box::new(error),
            })?;
        if markdown.len() as u64 > self.settings.max_markdown_bytes {
            return Err(IntegrationError::ResponseTooLarge {
                limit: self.settings.max_markdown_bytes,
            });
        }

        let base = if title.trim().is_empty() {
            match target.site {
                WikiSite::Wikipedia => "wikipedia",
                WikiSite::MinecraftWiki => "mcwiki",
            }
        } else {
            title.trim()
        };
        Ok(RenderedDocument {
            bytes: Bytes::from(markdown.into_bytes()),
            file_name: format!("{}.md", safe_file_name(base)),
            mime_type: "text/markdown; charset=utf-8".to_owned(),
        })
    }

    async fn screenshot_intro(
        &self,
        target: &WikiTarget,
    ) -> Result<RenderedDocument, IntegrationError> {
        let Some(screenshot) = &self.screenshot else {
            return Err(IntegrationError::NotConfigured {
                provider: "wiki screenshot browser".to_owned(),
            });
        };
        screenshot.capture(target.clone()).await
    }
}

fn summary_from_html(html: &str) -> WikiSummary {
    let document = Html::parse_document(html);
    let title = select_text(&document, "h1#firstHeading").unwrap_or_default();
    let Some(container) = select_first(&document, "#mw-content-text .mw-parser-output")
        .or_else(|| select_first(&document, "#content"))
    else {
        return WikiSummary {
            title,
            lead: String::new(),
        };
    };

    let mut lead = String::new();
    let mut paragraphs = 0;
    for child in container.children() {
        let Some(element) = scraper::ElementRef::wrap(child) else {
            continue;
        };
        if element.value().name() != "p" {
            continue;
        }
        let text = element.text().collect::<String>().trim().to_owned();
        if text.is_empty() {
            continue;
        }
        if !lead.is_empty() {
            lead.push_str("\n\n");
        }
        lead.push_str(&text);
        paragraphs += 1;
        if paragraphs >= 2 || lead.chars().count() >= 500 {
            break;
        }
    }

    WikiSummary { title, lead }
}

fn validate_target(site: WikiSite, url: &Url) -> Result<(), IntegrationError> {
    WikiTarget::parse(site, url.as_str()).map(|_| ())
}

fn select_first<'a>(document: &'a Html, selector: &str) -> Option<scraper::ElementRef<'a>> {
    let selector = Selector::parse(selector).ok()?;
    document.select(&selector).next()
}

fn select_text(document: &Html, selector: &str) -> Option<String> {
    select_first(document, selector)
        .map(|element| element.text().collect::<String>().trim().to_owned())
        .filter(|text| !text.is_empty())
}

fn safe_file_name(input: &str) -> String {
    let name = input
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let name = name.trim_matches(|character| matches!(character, '.' | ' '));
    if name.is_empty() {
        "page".to_owned()
    } else {
        name.chars().take(80).collect()
    }
}

fn map_http(error: HttpError) -> IntegrationError {
    match error {
        HttpError::Timeout => IntegrationError::Timeout,
        HttpError::BodyTooLarge { limit } => IntegrationError::ResponseTooLarge { limit },
        HttpError::Status { status, .. } => IntegrationError::Upstream {
            status: status.as_u16(),
        },
        other => IntegrationError::Transport {
            source: Box::new(other),
        },
    }
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
    fn prefers_the_nested_parser_output_over_the_outer_content() {
        let html = r#"
            <h1 id="firstHeading">僵尸</h1>
            <div id="content"><div id="mw-content-text">
              <div class="mw-parser-output"><p>第一段</p><p>第二段</p><p>第三段</p></div>
            </div></div>
        "#;
        let summary = summary_from_html(html);
        assert_eq!(summary.title, "僵尸");
        assert_eq!(summary.lead, "第一段\n\n第二段");
    }

    #[test]
    fn falls_back_to_content_when_parser_output_is_absent() {
        let html = r#"
            <h1 id="firstHeading">Title</h1>
            <div id="content"><p>Lead</p></div>
        "#;
        let summary = summary_from_html(html);
        assert_eq!(summary.title, "Title");
        assert_eq!(summary.lead, "Lead");
    }
}
