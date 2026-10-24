use std::time::Duration;

use super::{AppConfig, CompiledPlatforms, ConfigError, ConfigIssues};

impl AppConfig {
    /// Validates every section and reports all problems at once.
    pub fn validate(&self, compiled: CompiledPlatforms) -> Result<(), ConfigError> {
        let mut issues = Vec::new();

        if !(1..=300).contains(&self.app.shutdown_timeout_secs) {
            issues.push("app.shutdown_timeout_secs must be between 1 and 300".to_owned());
        }

        if self.logging.filter.trim().is_empty() {
            issues.push("logging.filter must not be empty".to_owned());
        }

        check_range(
            &mut issues,
            "runtime.inbound_queue_capacity",
            self.runtime.inbound_queue_capacity as u64,
            1,
            65_536,
        );
        check_range(
            &mut issues,
            "runtime.outbound_queue_capacity",
            self.runtime.outbound_queue_capacity as u64,
            1,
            65_536,
        );
        check_range(
            &mut issues,
            "runtime.handler_timeout_secs",
            self.runtime.handler_timeout_secs,
            1,
            3_600,
        );
        check_range(
            &mut issues,
            "runtime.send_timeout_secs",
            self.runtime.send_timeout_secs,
            1,
            600,
        );

        check_range(
            &mut issues,
            "http.connect_timeout_secs",
            self.http.connect_timeout_secs,
            1,
            120,
        );
        check_range(
            &mut issues,
            "http.request_timeout_secs",
            self.http.request_timeout_secs,
            1,
            600,
        );
        check_range(
            &mut issues,
            "http.max_response_bytes",
            self.http.max_response_bytes,
            1_024,
            1_073_741_824,
        );
        if self.http.user_agent.trim().is_empty() {
            issues.push("http.user_agent must not be empty".to_owned());
        }
        if let Some(proxy) = self.http.proxy_url() {
            let supported = ["http://", "https://", "socks5://", "socks5h://"];
            if !supported.iter().any(|prefix| proxy.starts_with(prefix)) {
                issues.push(
                    "http.proxy must start with http://, https://, socks5://, or socks5h://"
                        .to_owned(),
                );
            }
        }

        if self.storage.sqlite_path.as_os_str().is_empty() {
            issues.push("storage.sqlite_path must not be empty".to_owned());
        }
        check_range(
            &mut issues,
            "storage.max_connections",
            u64::from(self.storage.max_connections),
            1,
            64,
        );
        check_range(
            &mut issues,
            "storage.busy_timeout_ms",
            self.storage.busy_timeout_ms,
            0,
            60_000,
        );
        check_range(
            &mut issues,
            "storage.expiry_cleanup_interval_secs",
            self.storage.expiry_cleanup_interval_secs,
            0,
            86_400,
        );

        check_range(
            &mut issues,
            "media.max_download_bytes",
            self.media.max_download_bytes,
            1_024,
            1_073_741_824,
        );
        check_range(
            &mut issues,
            "media.max_upload_bytes",
            self.media.max_upload_bytes,
            1_024,
            1_073_741_824,
        );
        check_range(
            &mut issues,
            "media.max_thumbnail_bytes",
            self.media.max_thumbnail_bytes,
            1_024,
            1_073_741_824,
        );
        check_range(
            &mut issues,
            "media.max_image_pixels",
            self.media.max_image_pixels,
            1,
            1_000_000_000,
        );
        check_range(
            &mut issues,
            "media.max_concurrent_downloads",
            self.media.max_concurrent_downloads as u64,
            1,
            32,
        );
        check_range(
            &mut issues,
            "media.rendering.max_grid_posts",
            u64::from(self.media.rendering.max_grid_posts),
            1,
            12,
        );
        check_range(
            &mut issues,
            "media.rendering.max_render_pixels",
            self.media.rendering.max_render_pixels,
            1,
            100_000_000,
        );

        check_range(
            &mut issues,
            "web.request_timeout_secs",
            self.web.request_timeout_secs,
            1,
            120,
        );
        check_range(
            &mut issues,
            "web.max_html_bytes",
            self.web.max_html_bytes,
            1_024,
            1_073_741_824,
        );
        check_range(
            &mut issues,
            "web.max_markdown_bytes",
            self.web.max_markdown_bytes,
            1_024,
            1_073_741_824,
        );
        check_range(
            &mut issues,
            "web.max_redirects",
            u64::from(self.web.max_redirects),
            0,
            10,
        );
        if self.web.user_agent.trim().is_empty() {
            issues.push("web.user_agent must not be empty".to_owned());
        }
        check_range(
            &mut issues,
            "web.browser.page_load_timeout_secs",
            self.web.browser.page_load_timeout_secs,
            1,
            120,
        );
        check_range(
            &mut issues,
            "web.browser.job_timeout_secs",
            self.web.browser.job_timeout_secs,
            1,
            300,
        );
        check_range(
            &mut issues,
            "web.browser.queue_capacity",
            self.web.browser.queue_capacity as u64,
            1,
            64,
        );
        check_range(
            &mut issues,
            "web.browser.full_page_width",
            u64::from(self.web.browser.full_page_width),
            320,
            4_096,
        );
        check_range(
            &mut issues,
            "web.browser.full_page_max_height",
            u64::from(self.web.browser.full_page_max_height),
            320,
            32_768,
        );
        check_range(
            &mut issues,
            "web.browser.wiki_intro_width",
            u64::from(self.web.browser.wiki_intro_width),
            320,
            4_096,
        );
        if self.web.browser.enabled {
            let valid = url::Url::parse(&self.web.browser.webdriver_url).is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
            });
            if !valid {
                issues.push(
                    "web.browser.webdriver_url must be a valid http(s) URL with a host".to_owned(),
                );
            }
        }

        check_range(
            &mut issues,
            "integrations.e621.request_interval_ms",
            self.integrations.e621.request_interval_ms,
            0,
            60_000,
        );
        check_range(
            &mut issues,
            "integrations.e621.max_results_per_page",
            u64::from(self.integrations.e621.max_results_per_page),
            1,
            12,
        );
        if self.integrations.e621.user_agent.trim().is_empty() {
            issues.push("integrations.e621.user_agent must not be empty".to_owned());
        }

        if let Err(mut security_issues) = self.security.compile() {
            issues.append(&mut security_issues);
        }
        if let Err(mut policy_issues) = self.policy.compile() {
            issues.append(&mut policy_issues);
        }

        let telegram = &self.platforms.telegram;
        if telegram.enabled {
            if !compiled.telegram {
                issues.push(
                    "platforms.telegram.enabled requires the `telegram` build feature".to_owned(),
                );
            }
            if telegram.token().is_none_or(str::is_empty) {
                issues.push(
                    "platforms.telegram.token is required when Telegram is enabled".to_owned(),
                );
            }
        }

        let discord = &self.platforms.discord;
        if discord.enabled {
            if !compiled.discord {
                issues.push(
                    "platforms.discord.enabled requires the `discord` build feature".to_owned(),
                );
            }
            if discord.token().is_none_or(str::is_empty) {
                issues
                    .push("platforms.discord.token is required when Discord is enabled".to_owned());
            }
        }

        if self.commands.prefix.is_empty() {
            issues.push("commands.prefix must not be empty".to_owned());
        } else if self.commands.prefix.chars().all(char::is_whitespace) {
            issues.push("commands.prefix must not be only whitespace".to_owned());
        }
        if self.commands.prefix.chars().any(char::is_control) {
            issues.push("commands.prefix must not contain control characters".to_owned());
        }
        check_range(
            &mut issues,
            "commands.twenty_four.time_limit_secs",
            self.commands.twenty_four.time_limit_secs,
            1,
            3_600,
        );

        check_range(
            &mut issues,
            "integrations.whois.connect_timeout_secs",
            self.integrations.whois.connect_timeout_secs,
            1,
            30,
        );
        check_range(
            &mut issues,
            "integrations.whois.total_timeout_secs",
            self.integrations.whois.total_timeout_secs,
            1,
            30,
        );
        if self.integrations.whois.total_timeout_secs < self.integrations.whois.connect_timeout_secs
        {
            issues.push(
                "integrations.whois.total_timeout_secs must be greater than or equal to \
                 integrations.whois.connect_timeout_secs"
                    .to_owned(),
            );
        }
        check_range(
            &mut issues,
            "integrations.whois.max_response_bytes",
            self.integrations.whois.max_response_bytes,
            1_024,
            1_048_576,
        );

        if issues.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Invalid(ConfigIssues(issues)))
        }
    }

    /// Requires at least one platform before the application starts connecting.
    ///
    /// `check-config` deliberately does not require this, so the shipped example file can be
    /// validated with every platform disabled.
    pub fn require_platform(&self) -> Result<(), ConfigError> {
        if self.platforms.telegram.enabled || self.platforms.discord.enabled {
            Ok(())
        } else {
            Err(ConfigError::Invalid(ConfigIssues(vec![
                "at least one platform must be enabled to run".to_owned(),
            ])))
        }
    }

    /// Process-level durations derived from validated scalar configuration.
    pub fn shutdown_timeout(&self) -> Duration {
        Duration::from_secs(self.app.shutdown_timeout_secs)
    }
}

fn check_range(issues: &mut Vec<String>, field: &str, value: u64, min: u64, max: u64) {
    if !(min..=max).contains(&value) {
        issues.push(format!(
            "{field} must be between {min} and {max} (got {value})"
        ));
    }
}
