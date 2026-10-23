use std::time::Duration;

use chrono::{DateTime, Utc};

pub(crate) const MAX_REPLY_CHARS: usize = 1_800;

/// Truncates by Unicode scalar values, never by a byte index.
pub(crate) fn truncate_reply(text: &str) -> String {
    if text.chars().count() <= MAX_REPLY_CHARS {
        return text.to_owned();
    }

    const MARKER: &str = "\n…（结果过长，已截断）";
    let keep = MAX_REPLY_CHARS.saturating_sub(MARKER.chars().count());
    let mut output: String = text.chars().take(keep).collect();
    output.push_str(MARKER);
    output
}

pub(crate) fn strip_controls(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .collect()
}

pub(crate) fn metric(value: u64) -> String {
    const UNITS: [(u64, &str); 4] = [
        (1_000_000_000_000, "T"),
        (1_000_000_000, "B"),
        (1_000_000, "M"),
        (1_000, "k"),
    ];
    for (divisor, suffix) in UNITS {
        if value >= divisor {
            return format!("{:.1}{suffix}", value as f64 / divisor as f64);
        }
    }
    value.to_string()
}

pub(crate) fn duration_mmss(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

pub(crate) fn utc_time(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_counts_characters() {
        let text = "中".repeat(MAX_REPLY_CHARS + 10);
        let truncated = truncate_reply(&text);
        assert!(truncated.ends_with("已截断）"));
        assert!(truncated.chars().count() <= MAX_REPLY_CHARS);
    }

    #[test]
    fn metric_matches_jvm_style() {
        assert_eq!(metric(999), "999");
        assert_eq!(metric(1_000), "1.0k");
        assert_eq!(metric(1_500), "1.5k");
        assert_eq!(metric(1_000_000), "1.0M");
    }

    #[test]
    fn duration_is_mm_ss() {
        assert_eq!(duration_mmss(Duration::from_secs(213)), "03:33");
        assert_eq!(duration_mmss(Duration::from_secs(3_600)), "60:00");
    }
}
