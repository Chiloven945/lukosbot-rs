use std::collections::{BTreeMap, BTreeSet};

use lukosbot_domain::{ChatKind, InboundMessage, PlatformKind};

/// Conditions that must all match. `None` is a wildcard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolicyMatch {
    pub platform: Option<PlatformKind>,
    pub private_chat: Option<bool>,
    pub group: Option<bool>,
    pub nsfw: Option<bool>,
    pub chat_id: Option<i64>,
    pub user_id: Option<i64>,
}

impl PolicyMatch {
    pub(crate) fn matches(&self, context: &PolicyContext) -> bool {
        self.platform
            .is_none_or(|platform| platform == context.platform)
            && self
                .private_chat
                .is_none_or(|private_chat| private_chat == context.private_chat)
            && self.group.is_none_or(|group| group == context.group)
            && self.nsfw.is_none_or(|nsfw| nsfw == context.nsfw)
            && self
                .chat_id
                .is_none_or(|chat_id| Some(chat_id) == context.chat_id)
            && self
                .user_id
                .is_none_or(|user_id| Some(user_id) == context.user_id)
    }
}

/// One immutable policy rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRule {
    pub id: String,
    pub priority: i32,
    pub when: PolicyMatch,
    pub disable_commands: BTreeSet<String>,
    pub allow_values: BTreeMap<String, BTreeSet<String>>,
}

/// Context derived solely from the trusted inbound message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyContext {
    pub platform: PlatformKind,
    pub private_chat: bool,
    pub group: bool,
    pub nsfw: bool,
    pub chat_id: Option<i64>,
    pub user_id: Option<i64>,
}

impl PolicyContext {
    pub fn from_message(message: &InboundMessage) -> Self {
        let kind = message.address.kind;
        Self {
            platform: message.address.platform,
            private_chat: matches!(kind, ChatKind::Private),
            group: matches!(
                kind,
                ChatKind::Group | ChatKind::Supergroup | ChatKind::Channel
            ),
            nsfw: message.context.nsfw.unwrap_or(false),
            chat_id: message.address.chat_id.as_str().parse::<i64>().ok(),
            user_id: message.sender.id.as_str().parse::<i64>().ok(),
        }
    }
}

/// The result of evaluating command policy for one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDecision {
    pub allowed: bool,
    pub matched_rule_ids: Vec<String>,
}

impl PolicyDecision {
    pub fn allowed(matched_rule_ids: Vec<String>) -> Self {
        Self {
            allowed: true,
            matched_rule_ids,
        }
    }
}

pub(crate) fn normalize_command(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_lowercase())
    }
}

pub(crate) fn normalize_value(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_lowercase())
    }
}
