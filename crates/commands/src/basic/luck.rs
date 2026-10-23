use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Datelike, Local, NaiveDate};

use lukosbot_command_engine::{
    CommandContext, CommandDefinition, CommandError, CommandExecutor, CommandInvocation, InputSpec,
    UsageMetadata,
};

pub struct LuckCommand;

#[async_trait]
impl CommandExecutor for LuckCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        _invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        let today = Local::now().date_naive();
        let value = luck_value(context.sender().id.as_str(), today);
        context
            .reply_text(format!(
                "你今天的幸运值是……\n{}",
                luck_message(today, value)
            ))
            .await?;
        Ok(())
    }
}

pub fn definition() -> CommandDefinition {
    CommandDefinition::new(
        "luck",
        "获取今日幸运值",
        InputSpec::NoArgs,
        Arc::new(LuckCommand),
    )
    .aliases(["l", "jrrp"])
    .usage(UsageMetadata {
        examples: vec!["luck".to_owned()],
        ..UsageMetadata::default()
    })
}

/// Computes the JVM-compatible stable luck value for a user and local date.
pub fn luck_value(user_id: &str, date: NaiveDate) -> u8 {
    let user_seed = match user_id.parse::<i64>() {
        Ok(value) => java_long_hash(value),
        Err(_) => java_string_hash(user_id),
    } as i64;
    let date_seed = java_date_hash(date) as i64;

    stable_random(0, 100, &[user_seed, date_seed]) as u8
}

/// Produces the exact JVM wording for a date and luck value.
pub fn luck_message(date: NaiveDate, luck: u8) -> String {
    let special = match (date.month(), date.day()) {
        (1, 1) => Some("100！新年快乐！".to_owned()),
        (2, 29) => Some(format!("{luck}！是闰日诶！")),
        (5, 1) => Some("51！五一假期快乐！".to_owned()),
        (10, 1) => Some("100！国庆假期快乐！".to_owned()),
        (10, 31) => Some("100！万圣节快乐！".to_owned()),
        (12, 25) => Some("100！圣诞节快乐！".to_owned()),
        _ => None,
    };

    special.unwrap_or_else(|| match luck {
        0 => "嗯……呃……0……？".to_owned(),
        18 => "18！好耶满十八了！".to_owned(),
        4 | 44 => format!("{luck}……不死不死！"),
        27 => "27！爱吃什么？".to_owned(),
        50 => "50！是一半！".to_owned(),
        55 => "55！呜呜！".to_owned(),
        60 => "60！好耶及格了！".to_owned(),
        66 => "66！666 好耶！".to_owned(),
        67 => "sixty seven。嗯。".to_owned(),
        69 => "69？哇噻居然是 69！".to_owned(),
        78 => "咦惹，是 78……".to_owned(),
        88 => "88！诶！（被打）".to_owned(),
        91 => "91！恭喜！嗯。是的。".to_owned(),
        100 => "登登登！100！大满贯，大成功！".to_owned(),
        1..=10 => format!("{luck}……？仅供娱乐哦……至少比 {} 好！", luck - 1),
        11..=20 => format!("{luck}……？仅供娱乐哦！"),
        21..=49 => format!("{luck}？没关系捏，说不好明天更幸运！"),
        51..=75 => format!("{luck}！好耶，出门捡到一块钱！"),
        76..=89 => format!("{luck}！抽奖不做分母了！"),
        90..=95 => format!("{luck}！！哇噻，这就是传说中的欧皇吗？"),
        96..=99 => format!("居然是 {luck}！！！太棒了！"),
        _ => format!("{luck}？欸？不应该出现这个分数的，可能是出问题了捏……"),
    })
}

type JavaHash = i32;

fn java_long_hash(value: i64) -> JavaHash {
    let value = value as u64;
    ((value ^ (value >> 32)) as u32) as i32
}

fn java_string_hash(value: &str) -> JavaHash {
    value.encode_utf16().fold(0i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(unit))
    })
}

fn java_date_hash(date: NaiveDate) -> JavaHash {
    let year = date.year();
    let month = date.month() as i32;
    let day = date.day() as i32;
    let year_part = year & (0xFFFF_F800u32 as i32);
    let remainder = year
        .wrapping_shl(11)
        .wrapping_add(month.wrapping_shl(6))
        .wrapping_add(day);
    year_part ^ remainder
}

fn stable_random(min: i32, max: i32, seeds: &[i64]) -> i32 {
    const BASE: i64 = -0x61c8_8646_80b5_83eb;
    const MIX1: i64 = -0x40a7_b892_e31b_1a47;
    const MIX2: i64 = -0x6b2f_b644_ecce_ee15;

    let mut hash = BASE;
    for seed in seeds {
        let combined = seed
            .wrapping_add(BASE)
            .wrapping_add(hash.wrapping_shl(6))
            .wrapping_add(((hash as u64) >> 2) as i64);
        hash ^= combined;
    }

    hash ^= ((hash as u64) >> 30) as i64;
    hash = hash.wrapping_mul(MIX1);
    hash ^= ((hash as u64) >> 27) as i64;
    hash = hash.wrapping_mul(MIX2);
    hash ^= ((hash as u64) >> 31) as i64;

    let non_negative = (hash & 0x7FFF_FFFF) as i32;
    min + non_negative % (max - min + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn java_golden_vectors_match() {
        assert_eq!(luck_value("0", date(2026, 10, 23)), 30);
        assert_eq!(luck_value("42", date(2026, 10, 23)), 13);
        assert_eq!(luck_value("123456789", date(2026, 10, 23)), 98);
        assert_eq!(luck_value("987654321", date(2024, 2, 29)), 72);
        assert_eq!(luck_value("42", date(2026, 1, 1)), 10);
        assert_eq!(luck_value("0", date(2026, 12, 25)), 80);
    }

    #[test]
    fn same_user_and_day_are_stable() {
        let day = date(2026, 10, 23);
        assert_eq!(luck_value("123", day), luck_value("123", day));
    }

    #[test]
    fn special_dates_use_reference_copy() {
        assert_eq!(luck_message(date(2026, 1, 1), 3), "100！新年快乐！");
        assert_eq!(luck_message(date(2024, 2, 29), 72), "72！是闰日诶！");
        assert_eq!(luck_message(date(2026, 10, 31), 3), "100！万圣节快乐！");
        assert_eq!(luck_message(date(2026, 12, 25), 3), "100！圣诞节快乐！");
    }

    #[test]
    fn special_scores_use_reference_copy() {
        let day = date(2026, 10, 23);
        assert_eq!(luck_message(day, 0), "嗯……呃……0……？");
        assert_eq!(luck_message(day, 4), "4……不死不死！");
        assert_eq!(luck_message(day, 91), "91！恭喜！嗯。是的。");
    }

    #[test]
    fn non_numeric_platform_ids_still_work() {
        let value = luck_value("user-abc", date(2026, 10, 23));
        assert!(value <= 100);
    }
}
