use std::collections::{BTreeSet, HashSet};

use lukosbot_domain::InboundMessage;

use super::error::PolicyError;
use super::model::{PolicyContext, PolicyDecision, PolicyRule, normalize_command, normalize_value};

/// An immutable, pre-validated policy engine.
#[derive(Debug, Clone, Default)]
pub struct PolicyEngine {
    rules: Vec<PolicyRule>,
}

impl PolicyEngine {
    pub fn new(rules: Vec<PolicyRule>) -> Result<Self, PolicyError> {
        let mut ids = HashSet::new();
        let mut normalized = Vec::with_capacity(rules.len());

        for mut rule in rules {
            let id = rule.id.trim().to_owned();
            if id.is_empty() {
                return Err(PolicyError::InvalidRule {
                    id: "<empty>".to_owned(),
                    message: "id must not be empty".to_owned(),
                });
            }
            if !ids.insert(id.clone()) {
                return Err(PolicyError::InvalidRule {
                    id,
                    message: "id must be unique".to_owned(),
                });
            }
            rule.id = id;

            rule.disable_commands = rule
                .disable_commands
                .iter()
                .filter_map(|command| normalize_command(command))
                .collect();

            let mut allow_values = std::collections::BTreeMap::new();
            for (key, values) in rule.allow_values {
                let Some(normalized_key) = normalize_value(&key) else {
                    return Err(PolicyError::InvalidRule {
                        id: rule.id.clone(),
                        message: "allow_values keys must not be empty".to_owned(),
                    });
                };
                if allow_values.contains_key(&normalized_key) {
                    return Err(PolicyError::InvalidRule {
                        id: rule.id.clone(),
                        message: format!(
                            "allow_values contains duplicate normalized key `{normalized_key}`"
                        ),
                    });
                }

                let normalized_values = values
                    .iter()
                    .filter_map(|value| normalize_value(value))
                    .collect();
                allow_values.insert(normalized_key, normalized_values);
            }
            rule.allow_values = allow_values;

            normalized.push(rule);
        }

        normalized.sort_by_key(|rule| std::cmp::Reverse(rule.priority));
        Ok(Self { rules: normalized })
    }

    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn evaluate_command(&self, message: &InboundMessage, command_name: &str) -> PolicyDecision {
        let Some(command_name) = normalize_command(command_name) else {
            return PolicyDecision::allowed(Vec::new());
        };

        let context = PolicyContext::from_message(message);
        let matched: Vec<&PolicyRule> = self
            .rules
            .iter()
            .filter(|rule| rule.when.matches(&context))
            .collect();

        let disabled = matched
            .iter()
            .any(|rule| rule.disable_commands.contains(&command_name));

        PolicyDecision {
            allowed: !disabled,
            matched_rule_ids: matched.iter().map(|rule| rule.id.clone()).collect(),
        }
    }

    pub fn allowed_values(
        &self,
        message: &InboundMessage,
        key: &str,
        defaults: Option<&[String]>,
    ) -> BTreeSet<String> {
        let Some(key) = normalize_value(key) else {
            return normalize_defaults(defaults.unwrap_or_default());
        };

        let context = PolicyContext::from_message(message);
        let mut result = defaults.map(normalize_defaults);

        for rule in self.rules.iter().filter(|rule| rule.when.matches(&context)) {
            let Some(allowed) = rule.allow_values.get(&key) else {
                continue;
            };

            match result.as_mut() {
                Some(current) => current.retain(|value| allowed.contains(value)),
                None => result = Some(allowed.clone()),
            }
        }

        result.unwrap_or_default()
    }
}

fn normalize_defaults(defaults: &[String]) -> BTreeSet<String> {
    defaults
        .iter()
        .filter_map(|value| normalize_value(value))
        .collect()
}

/// Policy query surface used by the command engine.
///
/// The command engine depends on this port rather than on application configuration.
pub trait CommandPolicy: Send + Sync {
    fn evaluate_command(&self, message: &InboundMessage, command_name: &str) -> PolicyDecision;

    fn is_command_allowed(&self, message: &InboundMessage, command_name: &str) -> bool {
        self.evaluate_command(message, command_name).allowed
    }

    fn allowed_values(
        &self,
        message: &InboundMessage,
        key: &str,
        defaults: Option<&[String]>,
    ) -> BTreeSet<String>;
}

impl CommandPolicy for PolicyEngine {
    fn evaluate_command(&self, message: &InboundMessage, command_name: &str) -> PolicyDecision {
        PolicyEngine::evaluate_command(self, message, command_name)
    }

    fn allowed_values(
        &self,
        message: &InboundMessage,
        key: &str,
        defaults: Option<&[String]>,
    ) -> BTreeSet<String> {
        PolicyEngine::allowed_values(self, message, key, defaults)
    }
}

/// An explicit no-op policy used by tests and by deployments with no rules.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllPolicy;

impl CommandPolicy for AllowAllPolicy {
    fn evaluate_command(&self, _message: &InboundMessage, _command_name: &str) -> PolicyDecision {
        PolicyDecision::allowed(Vec::new())
    }

    fn allowed_values(
        &self,
        _message: &InboundMessage,
        _key: &str,
        defaults: Option<&[String]>,
    ) -> BTreeSet<String> {
        normalize_defaults(defaults.unwrap_or_default())
    }
}
