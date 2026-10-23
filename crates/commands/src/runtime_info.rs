use std::time::{Duration, Instant};

use chrono::{DateTime, Local};

/// Runtime facts available to commands without exposing configuration or SDK clients.
#[derive(Debug)]
pub struct RuntimeInfo {
    started_at: Instant,
    version: &'static str,
    os: &'static str,
    arch: &'static str,
}

impl RuntimeInfo {
    pub fn new(version: &'static str) -> Self {
        Self {
            started_at: Instant::now(),
            version,
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        }
    }

    pub fn with_started_at(version: &'static str, started_at: Instant) -> Self {
        Self {
            started_at,
            version,
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        }
    }

    pub fn version(&self) -> &'static str {
        self.version
    }

    pub fn os(&self) -> &'static str {
        self.os
    }

    pub fn arch(&self) -> &'static str {
        self.arch
    }

    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }

    pub fn status_text(&self) -> String {
        self.status_text_at(Local::now())
    }

    pub fn status_text_at(&self, now: DateTime<Local>) -> String {
        format!(
            "Pong！{}\nLukosBot {}\n运行时间：{}\n系统：{} {}",
            now.format("%Y/%m/%d %H:%M:%S"),
            self.version,
            format_uptime(self.uptime().as_secs()),
            self.os,
            self.arch
        )
    }
}

/// Formats uptime as `d:hh:mm:ss`.
pub fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    format!("{days}:{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn formats_uptime_like_the_reference() {
        assert_eq!(format_uptime(0), "0:00:00:00");
        assert_eq!(format_uptime(3_661), "0:01:01:01");
        assert_eq!(format_uptime(90_061), "1:01:01:01");
    }

    #[test]
    fn status_exposes_runtime_facts_without_jvm_fields() {
        let started = Instant::now() - Duration::from_secs(65);
        let info = RuntimeInfo::with_started_at("9.9.9", started);
        let now = Local.with_ymd_and_hms(2026, 10, 23, 12, 0, 0).unwrap();
        let text = info.status_text_at(now);

        assert!(text.contains("Pong！2026/10/23 12:00:00"));
        assert!(text.contains("LukosBot 9.9.9"));
        assert!(text.contains("运行时间：0:00:01:05"));
        assert!(!text.contains("Kotlin"));
        assert!(!text.contains("Spring"));
    }
}
