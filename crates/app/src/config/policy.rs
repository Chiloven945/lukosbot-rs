use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use serde::Deserialize;

use lukosbot_core::policy::{PolicyMatch, PolicyRule};
use lukosbot_domain::PlatformKind;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PolicySection {
    pub rules: Vec<PolicyRuleConfig>,
}

impl PolicySection {
    /// Compiles raw TOML rules into immutable core rules.
    pub fn compile(&self) -> Result<Vec<PolicyRule>, Vec<String>> {
        let mut issues = Vec::new();
        let mut ids = BTreeSet::new();
        let mut rules = Vec::with_capacity(self.rules.len());

        for (index, raw) in self.rules.iter().enumerate() {
            let id = raw.id.trim().to_owned();
            if id.is_empty() {
                issues.push(format!("policy.rules[{index}].id must not be empty"));
                continue;
            }
            if !ids.insert(id.clone()) {
                issues.push(format!("policy.rules[{index}].id `{id}` is duplicated"));
                continue;
            }

            let platform = match raw.when.platform.as_deref().map(str::trim) {
                Some(platform) if !platform.is_empty() => {
                    match PlatformKind::from_str(&platform.to_ascii_lowercase()) {
                        Ok(platform) => Some(platform),
                        Err(_) => {
                            issues.push(format!(
                                "policy.rules[{index}].when.platform is unknown: `{platform}`"
                            ));
                            None
                        }
                    }
                }
                _ => None,
            };

            let disable_commands = raw
                .disable_commands
                .iter()
                .filter_map(|command| normalize_policy_value(command))
                .collect();

            let mut allow_values = BTreeMap::new();
            for (raw_key, raw_values) in &raw.allow_values {
                let Some(key) = normalize_policy_value(raw_key) else {
                    issues.push(format!(
                        "policy.rules[{index}].allow_values contains an empty key"
                    ));
                    continue;
                };
                if allow_values.contains_key(&key) {
                    issues.push(format!(
                        "policy.rules[{index}].allow_values contains duplicate normalized key `{key}`"
                    ));
                    continue;
                }
                let values = raw_values
                    .iter()
                    .filter_map(|value| normalize_policy_value(value))
                    .collect();
                allow_values.insert(key, values);
            }

            rules.push(PolicyRule {
                id,
                priority: raw.priority,
                when: PolicyMatch {
                    platform,
                    private_chat: raw.when.private_chat,
                    group: raw.when.group,
                    nsfw: raw.when.nsfw,
                    chat_id: raw.when.chat_id,
                    user_id: raw.when.user_id,
                },
                disable_commands,
                allow_values,
            });
        }

        if issues.is_empty() {
            Ok(rules)
        } else {
            Err(issues)
        }
    }
}

fn normalize_policy_value(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_lowercase())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PolicyRuleConfig {
    pub id: String,
    pub priority: i32,
    pub when: PolicyMatchConfig,
    pub disable_commands: Vec<String>,
    pub allow_values: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PolicyMatchConfig {
    pub platform: Option<String>,
    pub private_chat: Option<bool>,
    pub group: Option<bool>,
    pub nsfw: Option<bool>,
    pub chat_id: Option<i64>,
    pub user_id: Option<i64>,
}
