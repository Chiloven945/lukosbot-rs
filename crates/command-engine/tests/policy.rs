use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio_util::sync::CancellationToken;

use lukosbot_command_engine::{
    ArgType, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, CommandMessageHandler, CommandRegistryBuilder, InputSpec, PositionalSpec,
    help::render_list_where,
};
use lukosbot_core::AppRuntime;
use lukosbot_core::RuntimeSettings;
use lukosbot_core::policy::{CommandPolicy, PolicyEngine, PolicyMatch, PolicyRule};
use lukosbot_core::testing::{MemoryStateStore, RecordingAdapter};
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, InboundPart, MessageContext, OutboundPart,
    PlatformChatId, PlatformKind, PlatformUserId, Sender,
};

struct Reply;

#[async_trait]
impl CommandExecutor for Reply {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        _invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        context.reply_text("ok").await?;
        Ok(())
    }
}

fn registry() -> Arc<lukosbot_command_engine::CommandRegistry> {
    let mut builder = CommandRegistryBuilder::new();
    builder
        .register(
            CommandDefinition::new("secret", "secret", InputSpec::NoArgs, Arc::new(Reply))
                .alias("s"),
        )
        .unwrap();
    builder
        .register(CommandDefinition::new(
            "parse",
            "parse",
            InputSpec::Args(ArgsSpec {
                positionals: vec![PositionalSpec::new("value", ArgType::String)],
                options: Vec::new(),
            }),
            Arc::new(Reply),
        ))
        .unwrap();
    builder
        .register(
            CommandDefinition::new("hidden", "hidden", InputSpec::NoArgs, Arc::new(Reply)).hidden(),
        )
        .unwrap();
    Arc::new(builder.build())
}

fn settings() -> RuntimeSettings {
    RuntimeSettings {
        inbound_capacity: 16,
        outbound_capacity: 16,
        handler_timeout: Duration::from_secs(2),
        send_timeout: Duration::from_secs(1),
        shutdown_timeout: Duration::from_secs(2),
    }
}

fn inbound(text: &str, kind: ChatKind) -> InboundMessage {
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
        parts: vec![InboundPart::Text(text.to_owned())],
        reply_to: None,
        quoted: None,
        context: MessageContext::default(),
    }
}

fn engine() -> Arc<dyn CommandPolicy> {
    let rule = PolicyRule {
        id: "group-disables".to_owned(),
        priority: 10,
        when: PolicyMatch {
            platform: Some(PlatformKind::Telegram),
            group: Some(true),
            ..PolicyMatch::default()
        },
        disable_commands: BTreeSet::from(["secret".to_owned(), "parse".to_owned()]),
        allow_values: Default::default(),
    };
    Arc::new(PolicyEngine::new(vec![rule]).unwrap())
}

async fn replies(texts: &[&str], kind: ChatKind, expected_sends: usize) -> Vec<String> {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    for text in texts {
        adapter.push_inbound(inbound(text, kind));
    }
    let handler = Arc::new(CommandMessageHandler::new("/", registry(), engine()).unwrap());
    let runtime = AppRuntime::builder(
        settings(),
        handler,
        Arc::new(MemoryStateStore::new()) as Arc<dyn lukosbot_core::StateStore>,
    )
    .add_platform(adapter.clone())
    .unwrap()
    .build();

    let cancel = CancellationToken::new();
    let shutdown = cancel.clone();
    let task =
        tokio::spawn(async move { runtime.run(async move { shutdown.cancelled().await }).await });
    adapter.wait_for_sends(expected_sends).await;
    let sent = adapter.sent();
    cancel.cancel();
    task.await.unwrap().unwrap();

    sent.iter()
        .map(|record| match &record.message.parts[0] {
            OutboundPart::Text(text) => text.clone(),
            _ => panic!("expected text"),
        })
        .collect()
}

#[tokio::test]
async fn policy_denies_before_argument_parsing_and_aliases_use_canonical_names() {
    let sent = replies(&["/secret", "/s", "/parse", "/unknown"], ChatKind::Group, 3).await;

    assert_eq!(sent.len(), 3);
    assert!(sent[0].contains("不允许使用此命令：secret"));
    assert!(sent[1].contains("不允许使用此命令：secret"));
    assert!(sent[2].contains("不允许使用此命令：parse"));
    assert!(!sent[2].contains("缺少必填参数"));
}

#[tokio::test]
async fn policy_allows_commands_in_a_non_matching_context() {
    let sent = replies(&["/secret", "/parse value"], ChatKind::Private, 2).await;
    assert_eq!(sent, vec!["ok", "ok"]);
}

#[tokio::test]
async fn help_list_filter_uses_a_caller_supplied_predicate() {
    let registry = registry();
    let rendered = render_list_where(&registry, "/", |definition| definition.name != "secret");

    assert!(rendered.contains("/parse"));
    assert!(!rendered.contains("/secret"));
    assert!(!rendered.contains("/hidden"));
}
