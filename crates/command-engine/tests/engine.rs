use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use lukosbot_command_engine::{
    ArgType, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, CommandMessageHandler, CommandRegistry, CommandRegistryBuilder, InputSpec,
    PositionalSpec, lexer, parse_args, parser,
};
use lukosbot_core::testing::{MemoryStateStore, RecordingAdapter};
use lukosbot_core::{AppRuntime, RuntimeSettings, StateStore};
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, InboundPart, OutboundPart, PlatformChatId, PlatformKind,
    PlatformUserId, Sender,
};

struct Noop;

#[async_trait]
impl CommandExecutor for Noop {
    async fn execute(
        &self,
        _context: &CommandContext<'_>,
        _invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        Ok(())
    }
}

struct Reply(&'static str);

#[async_trait]
impl CommandExecutor for Reply {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        _invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        context.reply_text(self.0).await?;
        Ok(())
    }
}

struct Echo;

#[async_trait]
impl CommandExecutor for Echo {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        context.reply_text(invocation.raw_tail).await?;
        Ok(())
    }
}

fn noop() -> Arc<dyn CommandExecutor> {
    Arc::new(Noop)
}

fn address() -> ChatAddress {
    ChatAddress::new(
        PlatformKind::Telegram,
        PlatformChatId::new("100").unwrap(),
        ChatKind::Private,
    )
}

fn inbound(text: &str) -> InboundMessage {
    InboundMessage {
        address: address(),
        sender: Sender {
            id: PlatformUserId::new("7").unwrap(),
            display_name: None,
            username: None,
        },
        message_id: None,
        received_at: Utc::now(),
        parts: vec![InboundPart::Text(text.to_owned())],
        reply_to: None,
    }
}

fn settings() -> RuntimeSettings {
    RuntimeSettings {
        inbound_capacity: 8,
        outbound_capacity: 8,
        handler_timeout: Duration::from_secs(1),
        send_timeout: Duration::from_secs(1),
        shutdown_timeout: Duration::from_secs(2),
    }
}

fn test_registry() -> CommandRegistry {
    let mut builder = CommandRegistryBuilder::new();
    builder
        .register(CommandDefinition::new(
            "ping",
            "pong",
            InputSpec::NoArgs,
            Arc::new(Reply("pong")),
        ))
        .unwrap();
    builder
        .register(CommandDefinition::new(
            "echo",
            "echo",
            InputSpec::Raw {
                name: "text".to_owned(),
                required: false,
            },
            Arc::new(Echo),
        ))
        .unwrap();
    builder
        .register(CommandDefinition::new(
            "dice",
            "dice",
            InputSpec::Args(ArgsSpec {
                positionals: vec![PositionalSpec::new("count", ArgType::I64)],
                options: Vec::new(),
            }),
            noop(),
        ))
        .unwrap();
    builder.build()
}

#[test]
fn lexer_matches_reference_quote_rules() {
    assert_eq!(
        lexer::split("a b c")
            .unwrap()
            .iter()
            .map(|token| token.value.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
    assert_eq!(lexer::split("\"a b\" c").unwrap()[0].value, "a b");
    assert_eq!(lexer::split("'a b' c").unwrap()[0].value, "a b");
    assert!(lexer::split("\"a b").is_err());
    assert_eq!(lexer::split("\"\"").unwrap().len(), 0);
    assert_eq!(lexer::split("--key=value").unwrap()[0].value, "--key=value");
}

#[test]
fn parser_handles_options_and_defaults() {
    let spec = ArgsSpec {
        positionals: vec![
            PositionalSpec::new("count", ArgType::I64)
                .optional()
                .default(lukosbot_command_engine::ArgValue::I64(1)),
        ],
        options: Vec::new(),
    };
    let tokens = lexer::split("").unwrap();
    let parsed = parse_args(&tokens, &spec).unwrap();
    assert_eq!(parsed.get::<i64>("count").unwrap(), 1);
}

#[test]
fn registry_rejects_duplicate_names_and_resolves_aliases() {
    let mut builder = CommandRegistryBuilder::new();
    builder
        .register(CommandDefinition::new(
            "ping",
            "pong",
            InputSpec::NoArgs,
            noop(),
        ))
        .unwrap();
    assert!(
        builder
            .register(CommandDefinition::new(
                "ping",
                "duplicate",
                InputSpec::NoArgs,
                noop(),
            ))
            .is_err()
    );

    let registry = builder.build();
    assert!(registry.find("PING").is_some());
}

#[test]
fn literal_child_wins_over_parent_fallback() {
    let mut builder = CommandRegistryBuilder::new();
    builder
        .register(
            CommandDefinition::new(
                "music",
                "music",
                InputSpec::Raw {
                    name: "query".to_owned(),
                    required: false,
                },
                noop(),
            )
            .child(lukosbot_command_engine::CommandNode::leaf(
                "link",
                "link",
                InputSpec::NoArgs,
                noop(),
            )),
        )
        .unwrap();
    let registry = builder.build();
    let resolved = registry.resolve("music link").unwrap().unwrap();
    assert_eq!(resolved.path, vec!["music", "link"]);
    assert_eq!(resolved.tail, "");
}

async fn run_messages(
    texts: &[&str],
    expected_sends: usize,
) -> (
    Vec<lukosbot_core::testing::SentRecord>,
    Arc<RecordingAdapter>,
) {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    for text in texts {
        adapter.push_inbound(inbound(text));
    }

    let handler = Arc::new(CommandMessageHandler::new("/", Arc::new(test_registry())).unwrap());
    let runtime = AppRuntime::builder(
        settings(),
        handler,
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    )
    .add_platform(adapter.clone())
    .unwrap()
    .build();

    let cancel = CancellationToken::new();
    let shutdown = cancel.clone();
    let task =
        tokio::spawn(async move { runtime.run(async move { shutdown.cancelled().await }).await });

    if expected_sends == 0 {
        tokio::time::sleep(Duration::from_millis(50)).await;
    } else {
        adapter.wait_for_sends(expected_sends).await;
    }
    let sent = adapter.sent();
    cancel.cancel();
    timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");
    (sent, adapter)
}

#[tokio::test]
async fn handler_ignores_bare_and_unknown_commands() {
    let (sent, _) = run_messages(&["ping", "/unknown"], 0).await;
    assert!(sent.is_empty());
}

#[tokio::test]
async fn handler_dispatches_no_args_and_preserves_raw_tail() {
    let (sent, _) = run_messages(&["/ping", "/echo hello   world"], 2).await;
    assert_eq!(sent.len(), 2);
    assert_eq!(
        sent[0].message.parts,
        vec![OutboundPart::Text("pong".to_owned())]
    );
    assert_eq!(
        sent[1].message.parts,
        vec![OutboundPart::Text("hello   world".to_owned())]
    );
}

#[tokio::test]
async fn handler_renders_parse_errors() {
    let (sent, _) = run_messages(&["/ping extra", "/dice abc"], 2).await;
    assert_eq!(sent.len(), 2);
    let first = match &sent[0].message.parts[0] {
        OutboundPart::Text(text) => text,
        _ => panic!("expected text"),
    };
    assert!(first.contains("不需要参数"));
    assert!(first.contains("/help ping"));
    let second = match &sent[1].message.parts[0] {
        OutboundPart::Text(text) => text,
        _ => panic!("expected text"),
    };
    assert!(second.contains("需要整数"));
}

#[tokio::test]
async fn handler_replies_to_blank_raw_command() {
    let (sent, _) = run_messages(&["/echo"], 1).await;
    // The test Echo executor returns an empty string, which is still a valid reply.
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].message.parts,
        vec![OutboundPart::Text(String::new())]
    );
}

#[test]
fn parser_module_alias_is_public() {
    // Keep the public parser entry point covered by a compile-time use.
    let _ = parser::parse_args;
}

fn args_spec() -> ArgsSpec {
    ArgsSpec {
        positionals: vec![
            PositionalSpec::new("name", ArgType::String),
            PositionalSpec::new("rest", ArgType::String)
                .optional()
                .greedy(),
        ],
        options: vec![
            lukosbot_command_engine::OptionSpec::new("count", ["-c", "--count"], ArgType::I64),
            lukosbot_command_engine::OptionSpec::new("verbose", ["-v", "--verbose"], ArgType::Bool),
            lukosbot_command_engine::OptionSpec::new("providers", ["--providers"], ArgType::String)
                .split_by(","),
            lukosbot_command_engine::OptionSpec::new("tag", ["--tag"], ArgType::String)
                .repeatable(),
        ],
    }
}

fn parse(
    text: &str,
) -> Result<lukosbot_command_engine::ParsedArgs, lukosbot_command_engine::CommandParseError> {
    let tokens = lexer::split(text).unwrap();
    parse_args(&tokens, &args_spec())
}

#[test]
fn parser_supports_option_forms_and_greedy_tail() {
    let parsed = parse("--count=3 --verbose alice hello world").unwrap();
    assert_eq!(parsed.get::<i64>("count").unwrap(), 3);
    assert!(parsed.get::<bool>("verbose").unwrap());
    assert_eq!(parsed.get::<String>("name").unwrap(), "alice");
    assert_eq!(parsed.get::<String>("rest").unwrap(), "hello world");

    let parsed = parse("-c 4 -v alice").unwrap();
    assert_eq!(parsed.get::<i64>("count").unwrap(), 4);
    assert!(parsed.get::<bool>("verbose").unwrap());
}

#[test]
fn parser_handles_split_lists_and_negative_numbers() {
    let parsed = parse("--providers=a,b --count -3 alice").unwrap();
    assert_eq!(
        parsed.get::<Vec<String>>("providers").unwrap(),
        vec!["a".to_owned(), "b".to_owned()]
    );
    assert_eq!(parsed.get::<i64>("count").unwrap(), -3);
}

#[test]
fn parser_accumulates_repeatable_options_and_parses_false_flags() {
    let parsed = parse("--tag a --tag=b --verbose=false alice").unwrap();
    assert_eq!(
        parsed.get::<Vec<String>>("tag").unwrap(),
        vec!["a".to_owned(), "b".to_owned()]
    );
    assert!(!parsed.get::<bool>("verbose").unwrap());
}

#[test]
fn parser_rejects_unknown_options_and_missing_values() {
    assert!(parse("--unknown alice").is_err());
    assert!(parse("--count").is_err());

    let spec = ArgsSpec {
        positionals: vec![PositionalSpec::new("name", ArgType::String)],
        options: Vec::new(),
    };
    let tokens = lexer::split("alice one").unwrap();
    assert!(parse_args(&tokens, &spec).is_err());
}

#[test]
fn parser_honors_double_dash() {
    let parsed = parse("alice -- --count").unwrap();
    assert_eq!(parsed.get::<String>("rest").unwrap(), "--count");
    assert!(!parsed.contains("count"));
}

#[test]
fn registry_rejects_invalid_argument_definitions() {
    let invalid_greedy = ArgsSpec {
        positionals: vec![
            PositionalSpec::new("first", ArgType::String).greedy(),
            PositionalSpec::new("second", ArgType::String),
        ],
        options: Vec::new(),
    };
    let mut builder = CommandRegistryBuilder::new();
    assert!(
        builder
            .register(CommandDefinition::new(
                "bad",
                "bad",
                InputSpec::Args(invalid_greedy),
                noop(),
            ))
            .is_err()
    );

    let invalid_option = ArgsSpec {
        positionals: Vec::new(),
        options: vec![lukosbot_command_engine::OptionSpec::new(
            "x",
            ["-toolong"],
            ArgType::String,
        )],
    };
    assert!(
        builder
            .register(CommandDefinition::new(
                "also-bad",
                "also bad",
                InputSpec::Args(invalid_option),
                noop(),
            ))
            .is_err()
    );
}

#[test]
fn root_and_child_aliases_resolve_case_insensitively() {
    let mut builder = CommandRegistryBuilder::new();
    builder
        .register(
            CommandDefinition::new("ping", "pong", InputSpec::NoArgs, noop())
                .alias("p")
                .child(
                    lukosbot_command_engine::CommandNode::leaf(
                        "now",
                        "now",
                        InputSpec::NoArgs,
                        noop(),
                    )
                    .alias("n"),
                ),
        )
        .unwrap();
    let registry = builder.build();

    let root = registry.resolve("P").unwrap().unwrap();
    assert_eq!(root.path, vec!["ping"]);

    let child = registry.resolve("PING N").unwrap().unwrap();
    assert_eq!(child.path, vec!["ping", "now"]);
}
