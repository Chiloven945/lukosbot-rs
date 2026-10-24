use std::collections::{BTreeMap, BTreeSet};

use chrono::Utc;

use lukosbot_core::policy::{AllowAllPolicy, CommandPolicy, PolicyEngine, PolicyMatch, PolicyRule};
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, InboundPart, MessageContext, PlatformChatId,
    PlatformKind, PlatformUserId, Sender,
};

fn message(kind: ChatKind, nsfw: Option<bool>) -> InboundMessage {
    InboundMessage {
        address: ChatAddress::new(
            PlatformKind::Telegram,
            PlatformChatId::new("100").unwrap(),
            kind,
        ),
        sender: Sender {
            id: PlatformUserId::new("7").unwrap(),
            display_name: None,
            username: None,
        },
        message_id: None,
        received_at: Utc::now(),
        parts: vec![InboundPart::Text("/test".to_owned())],
        reply_to: None,
        quoted: None,
        context: MessageContext::new(nsfw),
    }
}

fn rule(
    id: &str,
    priority: i32,
    when: PolicyMatch,
    disabled: &[&str],
    allow: BTreeMap<String, BTreeSet<String>>,
) -> PolicyRule {
    PolicyRule {
        id: id.to_owned(),
        priority,
        when,
        disable_commands: disabled.iter().map(|value| (*value).to_owned()).collect(),
        allow_values: allow,
    }
}

#[test]
fn disabled_commands_are_the_union_of_matching_rules() {
    let engine = PolicyEngine::new(vec![
        rule(
            "high",
            100,
            PolicyMatch::default(),
            &["alpha"],
            BTreeMap::new(),
        ),
        rule(
            "low",
            1,
            PolicyMatch {
                platform: Some(PlatformKind::Telegram),
                private_chat: Some(false),
                ..PolicyMatch::default()
            },
            &["beta"],
            BTreeMap::new(),
        ),
    ])
    .unwrap();

    let group = message(ChatKind::Group, None);
    assert!(!engine.is_command_allowed(&group, "alpha"));
    assert!(!engine.is_command_allowed(&group, "beta"));
    assert!(engine.is_command_allowed(&group, "gamma"));

    let private = message(ChatKind::Private, None);
    assert!(!engine.is_command_allowed(&private, "alpha"));
    assert!(engine.is_command_allowed(&private, "beta"));
}

#[test]
fn matching_uses_all_conditions_and_nsfw_defaults_to_false() {
    let engine = PolicyEngine::new(vec![rule(
        "nsfw",
        1,
        PolicyMatch {
            platform: Some(PlatformKind::Telegram),
            group: Some(true),
            nsfw: Some(false),
            ..PolicyMatch::default()
        },
        &["safe"],
        BTreeMap::new(),
    )])
    .unwrap();

    assert!(!engine.is_command_allowed(&message(ChatKind::Group, None), "safe"));
    assert!(!engine.is_command_allowed(&message(ChatKind::Group, Some(false)), "safe"));
    assert!(engine.is_command_allowed(&message(ChatKind::Group, Some(true)), "safe"));
    assert!(engine.is_command_allowed(&message(ChatKind::Private, None), "safe"));
}

#[test]
fn unknown_chat_kind_is_neither_private_nor_group() {
    let engine = PolicyEngine::new(vec![rule(
        "private-only",
        1,
        PolicyMatch {
            private_chat: Some(true),
            ..PolicyMatch::default()
        },
        &["blocked"],
        BTreeMap::new(),
    )])
    .unwrap();

    assert!(engine.is_command_allowed(&message(ChatKind::Unknown, None), "blocked"));
}

#[test]
fn allowed_values_are_intersected_and_empty_sets_are_meaningful() {
    let mut first = BTreeMap::new();
    first.insert(
        "rating".to_owned(),
        BTreeSet::from(["s".to_owned(), "q".to_owned()]),
    );
    let mut second = BTreeMap::new();
    second.insert("RATING".to_owned(), BTreeSet::from(["s".to_owned()]));
    let mut empty = BTreeMap::new();
    empty.insert("rating".to_owned(), BTreeSet::new());

    let engine = PolicyEngine::new(vec![
        rule("one", 10, PolicyMatch::default(), &[], first),
        rule("two", 5, PolicyMatch::default(), &[], second),
    ])
    .unwrap();

    let defaults = vec!["s".to_owned(), "q".to_owned(), "e".to_owned()];
    let allowed = engine.allowed_values(&message(ChatKind::Group, None), "Rating", Some(&defaults));
    assert_eq!(allowed, BTreeSet::from(["s".to_owned()]));

    let empty_engine =
        PolicyEngine::new(vec![rule("empty", 1, PolicyMatch::default(), &[], empty)]).unwrap();
    assert!(
        empty_engine
            .allowed_values(&message(ChatKind::Group, None), "rating", None)
            .is_empty()
    );
}

#[test]
fn missing_defaults_and_no_matching_rule_produce_an_empty_set() {
    let engine = PolicyEngine::empty();
    assert!(
        engine
            .allowed_values(&message(ChatKind::Group, None), "rating", None)
            .is_empty()
    );

    let defaults = vec!["s".to_owned(), "q".to_owned()];
    assert_eq!(
        engine.allowed_values(&message(ChatKind::Group, None), "rating", Some(&defaults)),
        BTreeSet::from(["s".to_owned(), "q".to_owned()])
    );
}

#[test]
fn duplicate_normalized_allow_values_keys_are_rejected() {
    let mut values = BTreeMap::new();
    values.insert("A".to_owned(), BTreeSet::from(["s".to_owned()]));
    values.insert("a".to_owned(), BTreeSet::from(["q".to_owned()]));

    let error = PolicyEngine::new(vec![rule(
        "duplicate",
        1,
        PolicyMatch::default(),
        &[],
        values,
    )])
    .unwrap_err();
    assert!(error.to_string().contains("duplicate"));
}

#[test]
fn allow_all_policy_is_explicit_and_empty() {
    let policy = AllowAllPolicy;
    let message = message(ChatKind::Group, None);

    assert!(policy.is_command_allowed(&message, "any"));
    assert!(
        policy
            .evaluate_command(&message, "any")
            .matched_rule_ids
            .is_empty()
    );
}
