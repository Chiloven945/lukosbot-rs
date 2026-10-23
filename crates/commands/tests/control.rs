use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;

use lukosbot_command_engine::CommandMessageHandler;
use lukosbot_commands::{
    BasicCommandsConfig, ControlCommands, RuntimeInfo, build_registry_with_controls,
};
use lukosbot_core::auth::{AuthorizationService, BotAdminService};
use lukosbot_core::policy::{AllowAllPolicy, CommandPolicy, PolicyEngine, PolicyMatch, PolicyRule};
use lukosbot_core::state::{
    StateDefinition, StateDefinitionError, StateMetadata, StateRegistry, StateService,
    TypedStateDefinition,
};
use lukosbot_core::testing::{MemoryStateStore, RecordingAdapter, SentRecord};
use lukosbot_core::{AppRuntime, RuntimeSettings, StateScopeKind, StateStore};
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, InboundPart, MessageContext, OutboundPart,
    PlatformChatId, PlatformKind, PlatformUserId, Sender,
};

fn inbound(text: &str, user_id: &str, kind: ChatKind) -> InboundMessage {
    InboundMessage {
        address: ChatAddress::new(
            PlatformKind::Telegram,
            PlatformChatId::new("100").unwrap(),
            kind,
        ),
        sender: Sender {
            id: PlatformUserId::new(user_id).unwrap(),
            display_name: None,
            username: None,
        },
        message_id: None,
        received_at: Utc::now(),
        parts: vec![InboundPart::Text(text.to_owned())],
        reply_to: None,
        context: MessageContext::default(),
    }
}

fn settings() -> RuntimeSettings {
    RuntimeSettings {
        inbound_capacity: 64,
        outbound_capacity: 64,
        handler_timeout: Duration::from_secs(2),
        send_timeout: Duration::from_secs(1),
        shutdown_timeout: Duration::from_secs(2),
    }
}

fn definition() -> Arc<dyn StateDefinition> {
    let metadata = StateMetadata::new("lang", "prefs", "回复语言")
        .allowed_scopes([
            StateScopeKind::User,
            StateScopeKind::Chat,
            StateScopeKind::Global,
        ])
        .preferred_scope(StateScopeKind::User)
        .suggestions(["zh".to_owned(), "en".to_owned()]);
    Arc::new(TypedStateDefinition::new(
        metadata,
        "zh".to_owned(),
        |raw| {
            let raw = raw.trim();
            if raw.is_empty() {
                Err(StateDefinitionError::parse("语言不能为空"))
            } else {
                Ok(raw.to_owned())
            }
        },
        |value| value.clone(),
    ))
}

struct Harness {
    adapter: Arc<RecordingAdapter>,
    cancel: CancellationToken,
    task:
        tokio::task::JoinHandle<Result<lukosbot_core::ShutdownReport, lukosbot_core::RuntimeError>>,
}

fn text_of(record: &SentRecord) -> String {
    match &record.message.parts[0] {
        OutboundPart::Text(text) => text.clone(),
        _ => panic!("expected text reply"),
    }
}

async fn start(
    messages: Vec<InboundMessage>,
    definitions: Vec<Arc<dyn StateDefinition>>,
    bootstrap: BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>,
) -> Harness {
    start_with_seed(
        messages,
        definitions,
        bootstrap,
        None,
        Arc::new(AllowAllPolicy),
    )
    .await
}

async fn start_with_policy(
    messages: Vec<InboundMessage>,
    definitions: Vec<Arc<dyn StateDefinition>>,
    bootstrap: BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>,
    policy: Arc<dyn CommandPolicy>,
) -> Harness {
    start_with_seed(messages, definitions, bootstrap, None, policy).await
}

async fn start_with_seed(
    messages: Vec<InboundMessage>,
    definitions: Vec<Arc<dyn StateDefinition>>,
    bootstrap: BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>,
    seed_admins: Option<serde_json::Value>,
    policy: Arc<dyn CommandPolicy>,
) -> Harness {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    for message in messages {
        adapter.push_inbound(message);
    }

    let store: Arc<dyn StateStore> = Arc::new(MemoryStateStore::new());
    if let Some(value) = seed_admins {
        store
            .put(
                lukosbot_core::StateKey::global("auth", "bot_admins").unwrap(),
                value,
                None,
            )
            .await
            .unwrap();
    }
    let admins = Arc::new(BotAdminService::new(Arc::clone(&store), bootstrap));
    let authorization = Arc::new(AuthorizationService::new(admins.clone(), Vec::new()));
    let state_registry = Arc::new(StateRegistry::new(definitions).unwrap());
    let state_service = Arc::new(StateService::new(Arc::clone(&store), state_registry));

    let registry = build_registry_with_controls(
        &BasicCommandsConfig {
            twenty_four: false,
            ..BasicCommandsConfig::default()
        },
        Arc::new(RuntimeInfo::new("test")),
        None,
        ControlCommands {
            admin: true,
            pref: true,
            bot_admins: Some(admins),
            authorization: Some(authorization),
            state_service: Some(state_service),
        },
    )
    .unwrap();

    let handler = Arc::new(CommandMessageHandler::new("/", Arc::new(registry), policy).unwrap());
    let runtime = AppRuntime::builder(settings(), handler, Arc::clone(&store))
        .add_platform(adapter.clone())
        .unwrap()
        .build();

    let cancel = CancellationToken::new();
    let shutdown = cancel.clone();
    let task =
        tokio::spawn(async move { runtime.run(async move { shutdown.cancelled().await }).await });

    Harness {
        adapter,
        cancel,
        task,
    }
}

async fn finish(harness: Harness) {
    harness.cancel.cancel();
    tokio::time::timeout(Duration::from_secs(3), harness.task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");
}

fn admin_bootstrap(user_id: &str) -> BTreeMap<PlatformKind, BTreeSet<PlatformUserId>> {
    BTreeMap::from([(
        PlatformKind::Telegram,
        BTreeSet::from([PlatformUserId::new(user_id).unwrap()]),
    )])
}

#[tokio::test]
async fn admin_commands_manage_dynamic_admins_and_show_identity() {
    let harness = start(
        vec![
            inbound("/admin me", "7", ChatKind::Private),
            inbound("/admin list", "7", ChatKind::Private),
            inbound("/admin add discord 42", "7", ChatKind::Private),
            inbound("/admin add discord 42", "7", ChatKind::Private),
            inbound("/admin list", "7", ChatKind::Private),
            inbound("/admin remove discord 42", "7", ChatKind::Private),
            inbound("/admin list", "7", ChatKind::Private),
        ],
        Vec::new(),
        admin_bootstrap("7"),
    )
    .await;

    harness.adapter.wait_for_sends(7).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();

    assert!(replies[0].contains("机器人管理员：是"));
    assert!(replies[1].contains("telegram: 7"));
    assert!(replies[2].contains("已添加机器人管理员：discord:42"));
    assert!(replies[3].contains("已经是动态机器人管理员"));
    assert!(replies[4].contains("discord: 42"));
    assert!(replies[5].contains("已移除动态机器人管理员：discord:42"));
    assert!(!replies[6].contains("discord: 42"));
    finish(harness).await;
}

#[tokio::test]
async fn non_admin_cannot_use_admin_management_commands() {
    let harness = start(
        vec![
            inbound("/admin me", "8", ChatKind::Private),
            inbound("/admin list", "8", ChatKind::Private),
            inbound("/admin add telegram 8", "8", ChatKind::Private),
        ],
        Vec::new(),
        admin_bootstrap("7"),
    )
    .await;

    harness.adapter.wait_for_sends(3).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();

    assert!(replies[0].contains("机器人管理员：否"));
    assert!(replies[1].contains("权限不足"));
    assert!(replies[2].contains("权限不足"));
    finish(harness).await;
}

#[tokio::test]
async fn removing_the_last_dynamic_admin_is_protected() {
    let harness = start_with_seed(
        vec![inbound("/admin remove telegram 7", "7", ChatKind::Private)],
        Vec::new(),
        BTreeMap::new(),
        Some(serde_json::json!({"telegram": ["7"]})),
        Arc::new(AllowAllPolicy),
    )
    .await;

    harness.adapter.wait_for_sends(1).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();
    assert!(replies[0].contains("无法移除最后一个机器人管理员"));
    finish(harness).await;
}

#[tokio::test]
async fn pref_commands_resolve_and_authorize_scopes() {
    let harness = start(
        vec![
            inbound("/pref list", "7", ChatKind::Private),
            inbound("/pref get lang", "7", ChatKind::Private),
            inbound("/pref set user lang en", "7", ChatKind::Private),
            inbound("/pref get user lang", "7", ChatKind::Private),
            inbound("/pref set global lang zh", "7", ChatKind::Private),
            inbound("/pref get global lang", "7", ChatKind::Private),
            inbound("/pref clear user lang", "7", ChatKind::Private),
            inbound("/pref get lang", "7", ChatKind::Private),
        ],
        vec![definition()],
        admin_bootstrap("7"),
    )
    .await;

    harness.adapter.wait_for_sends(8).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();

    assert!(replies[0].contains("lang"));
    assert!(replies[1].contains("当前生效值：zh"));
    assert!(replies[2].contains("设置为：en"));
    assert!(replies[3].contains("值：en"));
    assert!(replies[4].contains("设置为：zh"));
    assert!(replies[5].contains("值：zh"));
    assert!(replies[6].contains("已清除"));
    assert!(replies[7].contains("当前生效值：zh"));
    finish(harness).await;
}

#[tokio::test]
async fn pref_user_scope_is_open_but_global_scope_requires_bot_admin() {
    let harness = start(
        vec![
            inbound("/pref set user lang en", "8", ChatKind::Private),
            inbound("/pref set global lang en", "8", ChatKind::Private),
        ],
        vec![definition()],
        admin_bootstrap("7"),
    )
    .await;

    harness.adapter.wait_for_sends(2).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();

    assert!(replies[0].contains("设置为：en"));
    assert!(replies[1].contains("权限不足"));
    finish(harness).await;
}

#[tokio::test]
async fn help_and_policy_agree_for_group_denials() {
    let policy = Arc::new(
        PolicyEngine::new(vec![PolicyRule {
            id: "hide-admin".to_owned(),
            priority: 1,
            when: PolicyMatch {
                platform: Some(PlatformKind::Telegram),
                group: Some(true),
                ..PolicyMatch::default()
            },
            disable_commands: BTreeSet::from(["admin".to_owned()]),
            allow_values: BTreeMap::new(),
        }])
        .unwrap(),
    );

    let harness = start_with_policy(
        vec![
            inbound("/help", "7", ChatKind::Group),
            inbound("/help admin", "7", ChatKind::Group),
            inbound("/admin me", "7", ChatKind::Group),
        ],
        Vec::new(),
        admin_bootstrap("7"),
        policy,
    )
    .await;

    harness.adapter.wait_for_sends(3).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();

    assert!(replies[0].contains("/pref"));
    assert!(!replies[0].contains("/admin"));
    assert!(replies[1].contains("不允许使用此命令：admin"));
    assert!(replies[2].contains("不允许使用此命令：admin"));
    finish(harness).await;
}

#[tokio::test]
async fn pref_chat_scope_is_available_to_a_bot_admin() {
    let harness = start(
        vec![
            inbound("/pref set chat lang en", "7", ChatKind::Private),
            inbound("/pref get chat lang", "7", ChatKind::Private),
            inbound("/pref clear chat lang", "7", ChatKind::Private),
        ],
        vec![definition()],
        admin_bootstrap("7"),
    )
    .await;

    harness.adapter.wait_for_sends(3).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();

    assert!(replies[0].contains("设置为：en"));
    assert!(replies[1].contains("值：en"));
    assert!(replies[2].contains("已清除"));
    finish(harness).await;
}

#[tokio::test]
async fn empty_state_registry_reports_no_available_preferences() {
    let harness = start(
        vec![inbound("/pref list", "7", ChatKind::Private)],
        Vec::new(),
        admin_bootstrap("7"),
    )
    .await;

    harness.adapter.wait_for_sends(1).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();
    assert_eq!(replies[0], "暂无可用配置项。");
    finish(harness).await;
}
