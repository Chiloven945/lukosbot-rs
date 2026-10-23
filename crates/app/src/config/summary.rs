use std::fmt;

use super::AppConfig;

/// Human-readable configuration summary. Secrets are intentionally not part of it.
pub struct ConfigSummary<'a>(pub &'a AppConfig);

impl fmt::Display for ConfigSummary<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let config = self.0;
        writeln!(f, "environment: {}", config.app.environment)?;
        writeln!(
            f,
            "logging: {} ({})",
            config.logging.format, config.logging.filter
        )?;
        writeln!(
            f,
            "runtime: inbound_queue={} outbound_queue={} handler_timeout={}s send_timeout={}s",
            config.runtime.inbound_queue_capacity,
            config.runtime.outbound_queue_capacity,
            config.runtime.handler_timeout_secs,
            config.runtime.send_timeout_secs
        )?;
        writeln!(
            f,
            "http: connect={}s request={}s max_response={}B proxy={}",
            config.http.connect_timeout_secs,
            config.http.request_timeout_secs,
            config.http.max_response_bytes,
            if config.http.proxy_url().is_some() {
                "configured"
            } else {
                "none"
            }
        )?;
        writeln!(
            f,
            "storage: path={} max_connections={} expiry_cleanup={}s",
            config.storage.sqlite_path.display(),
            config.storage.max_connections,
            config.storage.expiry_cleanup_interval_secs
        )?;
        writeln!(
            f,
            "platforms: telegram={} discord={}",
            config.platforms.telegram.enabled, config.platforms.discord.enabled
        )?;
        writeln!(
            f,
            "security: bootstrap_admins={}",
            config
                .security
                .bootstrap_bot_admins
                .values()
                .map(Vec::len)
                .sum::<usize>()
        )?;
        writeln!(f, "policy: rules={}", config.policy.rules.len())?;
        writeln!(
            f,
            "integrations: github_token={} spotify={} soundcloud={} whois_timeouts={}s/{}s",
            if config.integrations.github.has_token() {
                "configured"
            } else {
                "none"
            },
            if config.integrations.music.spotify.available() {
                "configured"
            } else {
                "unavailable"
            },
            if config.integrations.music.soundcloud.available() {
                "configured"
            } else {
                "unavailable"
            },
            config.integrations.whois.connect_timeout_secs,
            config.integrations.whois.total_timeout_secs
        )?;
        writeln!(
            f,
            "commands: prefix={} ping={} echo={} coin={} dice={} luck={} twenty_four={} twenty_four_time_limit={}s weather={} admin={} pref={} service={}",
            config.commands.prefix,
            config.commands.basic.ping,
            config.commands.basic.echo,
            config.commands.basic.coin,
            config.commands.basic.dice,
            config.commands.basic.luck,
            config.commands.basic.twenty_four,
            config.commands.twenty_four.time_limit_secs,
            config.commands.basic.weather,
            config.commands.control.admin,
            config.commands.control.pref,
            config.commands.control.service
        )?;
        writeln!(
            f,
            "integration_commands: github={} bilibili={} ip={} music={} whois={}",
            config.commands.integrations.github,
            config.commands.integrations.bilibili,
            config.commands.integrations.ip,
            config.commands.integrations.music,
            config.commands.integrations.whois
        )?;
        Ok(())
    }
}
