use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;

use lukosbot_command_engine::CommandMessageHandler;
use lukosbot_commands::basic::twenty_four::TwentyFourService;
use lukosbot_commands::{BasicCommandsConfig, RuntimeInfo, build_registry};
use lukosbot_core::testing::{MemoryStateStore, RecordingAdapter};
use lukosbot_core::{AppRuntime, RuntimeSettings, StateStore};
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, InboundPart, OutboundPart, PlatformChatId, PlatformKind,
    PlatformUserId, Sender,
};

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
        inbound_capacity: 16,
        outbound_capacity: 16,
        handler_timeout: Duration::from_secs(2),
        send_timeout: Duration::from_secs(1),
        shutdown_timeout: Duration::from_secs(2),
    }
}

struct Harness {
    adapter: Arc<RecordingAdapter>,
    cancel: CancellationToken,
    task:
        tokio::task::JoinHandle<Result<lukosbot_core::ShutdownReport, lukosbot_core::RuntimeError>>,
}

fn text_of(record: &lukosbot_core::testing::SentRecord) -> String {
    match &record.message.parts[0] {
        OutboundPart::Text(text) => text.clone(),
        _ => panic!("expected a text reply"),
    }
}

async fn start_runtime(texts: &[&str], twenty_four_timeout: Option<Duration>) -> Harness {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    for text in texts {
        adapter.push_inbound(inbound(text));
    }

    let runtime_info = Arc::new(RuntimeInfo::new("test-version"));
    let (handle, worker) = match twenty_four_timeout {
        Some(timeout) => {
            let (handle, worker) = TwentyFourService::new(timeout).into_parts();
            (Some(handle), Some(worker))
        }
        None => (None, None),
    };

    let mut config = BasicCommandsConfig::default();
    if twenty_four_timeout.is_none() {
        config.twenty_four = false;
    }
    let registry = build_registry(&config, runtime_info, handle).unwrap();
    let handler = Arc::new(CommandMessageHandler::new("/", Arc::new(registry)).unwrap());
    let mut builder = AppRuntime::builder(
        settings(),
        handler,
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    );
    if let Some(worker) = worker {
        builder = builder.add_worker(Arc::new(worker)).unwrap();
    }
    let runtime = builder.add_platform(adapter.clone()).unwrap().build();

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

#[tokio::test]
async fn basic_commands_are_reachable_through_the_prefix_handler() {
    let harness = start_runtime(
        &[
            "ping",
            "/ping",
            "/echo hello   world",
            "/dice 3",
            "/luck",
            "/jrrp",
            "/help",
            "/start",
            "/weather",
            "/unknown",
        ],
        None,
    )
    .await;
    harness.adapter.wait_for_sends(8).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();
    assert_eq!(replies.len(), 8);
    assert!(replies[0].contains("Pong！"));
    assert_eq!(replies[1], "hello   world");
    assert!(replies[2].contains("你掷了 3 个骰子。"));
    assert!(replies[3].starts_with("你今天的幸运值是……"));
    assert_eq!(replies[3], replies[4]);
    assert!(replies[5].contains("/luck (l, jrrp)"));
    assert!(!replies[5].contains("/start"));
    assert!(replies[6].contains("欢迎使用 LukosBot"));
    assert_eq!(replies[7], "天气查询功能尚未接入，请关注后续更新。");
    finish(harness).await;
}

#[tokio::test]
async fn help_rejects_hidden_and_unknown_commands() {
    let harness = start_runtime(&["/help start", "/help whois", "/help luck"], None).await;
    harness.adapter.wait_for_sends(3).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();
    assert!(replies[0].contains("未知的命令：start"));
    assert!(replies[1].contains("未知的命令：whois"));
    assert!(replies[2].contains("命令：/luck"));
    assert!(replies[2].contains("别名：l, jrrp"));
    finish(harness).await;
}

#[tokio::test]
async fn twenty_four_timeout_is_delivered_to_the_original_chat() {
    let harness = start_runtime(&["/24"], Some(Duration::from_millis(60))).await;
    harness.adapter.wait_for_sends(2).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();
    assert!(replies[0].contains("新的一局 24 点开始了"));
    assert!(replies[1].contains("24 点游戏时间到了"));
    assert_eq!(harness.adapter.sent()[1].address, address());
    finish(harness).await;
}

#[tokio::test]
async fn command_errors_are_reported_through_the_handler() {
    let harness = start_runtime(
        &[
            "/echo",
            "/dice abc",
            "/ping extra",
            "/help luck img",
            "/help luck nope",
        ],
        None,
    )
    .await;
    harness.adapter.wait_for_sends(5).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();

    assert!(replies[0].contains("请输入要回显的文本"));
    assert!(replies[1].contains("需要整数"));
    assert!(replies[2].contains("不需要参数"));
    assert!(replies[3].contains("图片版帮助尚未实现"));
    assert!(replies[4].contains("未知的输出方式"));
    finish(harness).await;
}

#[tokio::test]
async fn aliases_and_coin_are_registered() {
    let harness = start_runtime(&["/h", "/l", "/coin 3"], None).await;
    harness.adapter.wait_for_sends(3).await;
    let replies: Vec<String> = harness.adapter.sent().iter().map(text_of).collect();

    assert!(replies[0].contains("可用命令："));
    assert!(replies[1].starts_with("你今天的幸运值是……"));
    assert!(replies[2].contains("你抛了 3 个硬币。"));
    finish(harness).await;
}
