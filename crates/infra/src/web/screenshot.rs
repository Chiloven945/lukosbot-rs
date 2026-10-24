use std::io::Cursor;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use image::ImageFormat;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::{Instant, sleep, timeout_at};

use lukosbot_core::error::TaskFailure;
use lukosbot_core::runtime::{RuntimeWorker, WorkerContext};
use lukosbot_integrations::{IntegrationError, RenderedDocument, WikiSite, WikiTarget};
use thirtyfour::ChromiumLikeCapabilities;
use thirtyfour::prelude::{DesiredCapabilities, WebDriver};
use thirtyfour::session::scriptret::ScriptRet;

const MEASURE_INTRO: &str = include_str!("scripts/wiki_intro_measure.js");

#[derive(Debug, Clone)]
pub struct ScreenshotSettings {
    pub webdriver_url: String,
    pub page_load_timeout: Duration,
    pub job_timeout: Duration,
    pub queue_capacity: usize,
    pub wiki_intro_width: u32,
    pub intro_height: u32,
}

impl Default for ScreenshotSettings {
    fn default() -> Self {
        Self {
            webdriver_url: "http://127.0.0.1:4444".to_owned(),
            page_load_timeout: Duration::from_secs(20),
            job_timeout: Duration::from_secs(25),
            queue_capacity: 4,
            wiki_intro_width: 1380,
            intro_height: 1_600,
        }
    }
}

#[derive(Clone)]
pub struct ScreenshotHandle {
    sender: mpsc::Sender<ScreenshotJob>,
}

impl ScreenshotHandle {
    pub async fn capture(&self, target: WikiTarget) -> Result<RenderedDocument, IntegrationError> {
        let (reply, receiver) = oneshot::channel();
        self.sender
            .try_send(ScreenshotJob { target, reply })
            .map_err(|_| IntegrationError::Unavailable {
                message: "screenshot queue is busy or closed".to_owned(),
            })?;
        receiver.await.map_err(|_| IntegrationError::Unavailable {
            message: "screenshot worker stopped".to_owned(),
        })?
    }
}

struct ScreenshotJob {
    target: WikiTarget,
    reply: oneshot::Sender<Result<RenderedDocument, IntegrationError>>,
}

pub struct ScreenshotWorker {
    receiver: Mutex<mpsc::Receiver<ScreenshotJob>>,
    settings: ScreenshotSettings,
}

impl ScreenshotWorker {
    pub fn new(settings: ScreenshotSettings) -> (ScreenshotHandle, Self) {
        let (sender, receiver) = mpsc::channel(settings.queue_capacity.max(1));
        (
            ScreenshotHandle { sender },
            Self {
                receiver: Mutex::new(receiver),
                settings,
            },
        )
    }
}

#[async_trait]
impl RuntimeWorker for ScreenshotWorker {
    fn name(&self) -> &'static str {
        "screenshot"
    }

    async fn run(&self, context: WorkerContext) -> Result<(), TaskFailure> {
        let mut receiver = self.receiver.lock().await;
        loop {
            tokio::select! {
                _ = context.cancel().cancelled() => {
                    while let Ok(job) = receiver.try_recv() {
                        let _ = job.reply.send(Err(IntegrationError::Unavailable {
                            message: "screenshot worker is shutting down".to_owned(),
                        }));
                    }
                    return Ok(());
                }
                job = receiver.recv() => {
                    let Some(job) = job else {
                        return Ok(());
                    };
                    let result = timeout_at(
                        Instant::now() + self.settings.job_timeout,
                        capture(&self.settings, &job.target),
                    )
                    .await
                    .unwrap_or_else(|_| Err(IntegrationError::Timeout));
                    let _ = job.reply.send(result);
                }
            }
        }
    }
}

async fn capture(
    settings: &ScreenshotSettings,
    target: &WikiTarget,
) -> Result<RenderedDocument, IntegrationError> {
    let mut capabilities = DesiredCapabilities::chrome();
    capabilities
        .add_arg("--headless=new")
        .map_err(unavailable)?;
    capabilities.add_arg("--disable-gpu").map_err(unavailable)?;
    capabilities
        .add_arg("--disable-dev-shm-usage")
        .map_err(unavailable)?;

    let driver = WebDriver::builder(&settings.webdriver_url, capabilities)
        .request_timeout(settings.job_timeout)
        .await
        .map_err(unavailable)?;
    driver
        .set_page_load_timeout(settings.page_load_timeout)
        .await
        .map_err(unavailable)?;

    let result = async {
        driver
            .goto(target.url.as_str())
            .await
            .map_err(unavailable)?;

        let first_height = driver
            .execute(MEASURE_INTRO, Vec::new())
            .await
            .ok()
            .and_then(script_number)
            .unwrap_or(settings.intro_height)
            .clamp(1, 2_600);
        driver
            .set_window_rect(0, 0, settings.wiki_intro_width, first_height)
            .await
            .map_err(unavailable)?;
        sleep(Duration::from_millis(700)).await;

        let second_height = driver
            .execute(MEASURE_INTRO, Vec::new())
            .await
            .ok()
            .and_then(script_number)
            .unwrap_or(first_height)
            .clamp(1, 2_600);
        let target_height = first_height.max(second_height);
        if target_height > first_height {
            driver
                .set_window_rect(0, 0, settings.wiki_intro_width, target_height)
                .await
                .map_err(unavailable)?;
            sleep(Duration::from_millis(400)).await;
        }

        let png = driver.screenshot_as_png().await.map_err(unavailable)?;
        let image = image::load_from_memory(&png).map_err(unavailable)?;
        let mut output = Cursor::new(Vec::new());
        image
            .write_to(&mut output, ImageFormat::Jpeg)
            .map_err(unavailable)?;
        let title = target
            .title
            .clone()
            .or_else(|| {
                target
                    .url
                    .path_segments()
                    .and_then(Iterator::last)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| match target.site {
                WikiSite::Wikipedia => "wikipedia".to_owned(),
                WikiSite::MinecraftWiki => "mcwiki".to_owned(),
            });
        Ok(RenderedDocument {
            bytes: Bytes::from(output.into_inner()),
            file_name: format!("{}.jpg", safe_file_name(&title)),
            mime_type: "image/jpeg".to_owned(),
        })
    }
    .await;

    if let Err(error) = driver.quit().await {
        tracing::debug!(%error, "WebDriver quit failed");
    }
    result
}

fn script_number(result: ScriptRet) -> Option<u32> {
    script_value_number(result.json())
}

fn script_value_number(value: &serde_json::Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| {
            value
                .as_f64()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map(|value| value.round() as u32)
        })
}

fn unavailable(error: impl std::fmt::Display) -> IntegrationError {
    IntegrationError::Unavailable {
        message: format!("browser failure: {error}"),
    }
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
        "wiki".to_owned()
    } else {
        name.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_names_reject_paths() {
        assert_eq!(safe_file_name("../a:b"), "_a_b");
    }

    #[test]
    fn script_numbers_are_bounded() {
        assert_eq!(script_value_number(&serde_json::json!(1200)), Some(1200));
        assert_eq!(script_value_number(&serde_json::json!(12.6)), Some(13));
        assert_eq!(script_value_number(&serde_json::json!(-1)), None);
        assert_eq!(script_value_number(&serde_json::json!("bad")), None);
    }
}
