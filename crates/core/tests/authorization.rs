use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;

use lukosbot_core::StateStore;
use lukosbot_core::auth::{
    AdminError, AdminIdentity, AdminMutation, AuthorizationError, AuthorizationService,
    BotAdminService, ChatAdminLookupError, ChatAdminResolver,
};
use lukosbot_core::ports::StateKey;
use lukosbot_core::testing::MemoryStateStore;
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, InboundPart, MessageContext, PlatformChatId,
    PlatformKind, PlatformUserId, Sender,
};

#[derive(Clone, Copy)]
enum ResolverBehavior {
    Allow,
    Deny,
    Error,
}

struct FakeResolver {
    platform: PlatformKind,
    behavior: ResolverBehavior,
    calls: AtomicUsize,
}

impl FakeResolver {
    fn new(platform: PlatformKind, behavior: ResolverBehavior) -> Arc<Self> {
        Arc::new(Self {
            platform,
            behavior,
            calls: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl ChatAdminResolver for FakeResolver {
    fn supports(&self, platform: PlatformKind) -> bool {
        platform == self.platform
    }

    async fn is_chat_admin(&self, _message: &InboundMessage) -> Result<bool, ChatAdminLookupError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        match self.behavior {
            ResolverBehavior::Allow => Ok(true),
            ResolverBehavior::Deny => Ok(false),
            ResolverBehavior::Error => Err(ChatAdminLookupError::Unavailable),
        }
    }
}

fn message(kind: ChatKind, user_id: &str) -> InboundMessage {
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
        parts: vec![InboundPart::Text("/admin me".to_owned())],
        reply_to: None,
        context: MessageContext::default(),
    }
}

fn service(
    bootstrap: BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>,
) -> (Arc<dyn StateStore>, Arc<BotAdminService>) {
    let store: Arc<dyn StateStore> = Arc::new(MemoryStateStore::new());
    let service = Arc::new(BotAdminService::new(Arc::clone(&store), bootstrap));
    (store, service)
}

fn identity(user_id: &str) -> AdminIdentity {
    AdminIdentity::new(
        PlatformKind::Telegram,
        PlatformUserId::new(user_id).unwrap(),
    )
}

#[tokio::test]
async fn bootstrap_and_dynamic_admins_are_unioned_without_removing_bootstrap() {
    let bootstrap = BTreeMap::from([(
        PlatformKind::Telegram,
        BTreeSet::from([PlatformUserId::new("1").unwrap()]),
    )]);
    let (_store, admins) = service(bootstrap);

    assert!(admins.is_bot_admin(&identity("1")).await.unwrap());
    assert!(!admins.is_bot_admin(&identity("2")).await.unwrap());

    assert_eq!(
        admins.add_dynamic_admin(identity("2")).await.unwrap(),
        AdminMutation::Added
    );
    assert!(admins.is_bot_admin(&identity("2")).await.unwrap());
    assert_eq!(
        admins.remove_dynamic_admin(identity("1")).await.unwrap(),
        AdminMutation::NotPresent
    );
    assert!(admins.is_bot_admin(&identity("1")).await.unwrap());

    assert_eq!(
        admins.remove_dynamic_admin(identity("2")).await.unwrap(),
        AdminMutation::Removed
    );
    assert!(!admins.is_bot_admin(&identity("2")).await.unwrap());
}

#[tokio::test]
async fn legacy_admin_json_is_read_and_canonical_writes_are_used() {
    let (store, admins) = service(BTreeMap::new());
    store
        .put(
            StateKey::global("auth", "bot_admins").unwrap(),
            json!({"TELEGRAM": [123], "DISCORD": ["456"]}),
            None,
        )
        .await
        .unwrap();

    assert!(admins.is_bot_admin(&identity("123")).await.unwrap());
    assert!(
        admins
            .is_bot_admin(&AdminIdentity::new(
                PlatformKind::Discord,
                PlatformUserId::new("456").unwrap()
            ))
            .await
            .unwrap()
    );

    admins.add_dynamic_admin(identity("789")).await.unwrap();
    let stored = store
        .get(&StateKey::global("auth", "bot_admins").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.value,
        json!({"discord": ["456"], "telegram": ["123", "789"]})
    );
}

#[tokio::test]
async fn corrupt_admin_json_is_not_treated_as_an_empty_list() {
    let (store, admins) = service(BTreeMap::new());
    store
        .put(
            StateKey::global("auth", "bot_admins").unwrap(),
            json!([]),
            None,
        )
        .await
        .unwrap();

    let error = admins.is_bot_admin(&identity("7")).await.unwrap_err();
    assert!(matches!(error, AdminError::CorruptAdminList));
}

#[tokio::test]
async fn last_dynamic_admin_cannot_be_removed_without_bootstrap_admins() {
    let (_store, admins) = service(BTreeMap::new());
    admins.add_dynamic_admin(identity("7")).await.unwrap();

    let error = admins
        .remove_dynamic_admin(identity("7"))
        .await
        .unwrap_err();
    assert!(matches!(error, AdminError::LastAdminProtection));
    assert!(admins.is_bot_admin(&identity("7")).await.unwrap());
}

#[tokio::test]
async fn private_chat_does_not_call_the_chat_admin_resolver() {
    let (_store, admins) = service(BTreeMap::new());
    let resolver = FakeResolver::new(PlatformKind::Telegram, ResolverBehavior::Allow);
    let auth = AuthorizationService::new(admins, vec![resolver.clone()]);

    let context = auth
        .inspect(&message(ChatKind::Private, "7"))
        .await
        .unwrap();
    assert!(!context.bot_admin);
    assert!(!context.chat_admin);
    assert_eq!(resolver.calls(), 0);
}

#[tokio::test]
async fn resolver_true_false_and_error_have_distinct_outcomes() {
    let (_store, admins) = service(BTreeMap::new());

    let allow = FakeResolver::new(PlatformKind::Telegram, ResolverBehavior::Allow);
    let auth = AuthorizationService::new(admins.clone(), vec![allow.clone()]);
    let context = auth.inspect(&message(ChatKind::Group, "7")).await.unwrap();
    assert!(context.chat_admin);
    assert_eq!(allow.calls(), 1);

    let deny = FakeResolver::new(PlatformKind::Telegram, ResolverBehavior::Deny);
    let auth = AuthorizationService::new(admins.clone(), vec![deny]);
    let context = auth.inspect(&message(ChatKind::Group, "7")).await.unwrap();
    assert!(!context.chat_admin);

    let error = FakeResolver::new(PlatformKind::Telegram, ResolverBehavior::Error);
    let auth = AuthorizationService::new(admins, vec![error]);
    let error = auth
        .inspect(&message(ChatKind::Group, "7"))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        AuthorizationError::ChatAdminUnavailable(ChatAdminLookupError::Unavailable)
    ));
}

#[tokio::test]
async fn bot_admin_short_circuits_the_chat_admin_resolver() {
    let bootstrap = BTreeMap::from([(
        PlatformKind::Telegram,
        BTreeSet::from([PlatformUserId::new("7").unwrap()]),
    )]);
    let (_store, admins) = service(bootstrap);
    let resolver = FakeResolver::new(PlatformKind::Telegram, ResolverBehavior::Error);
    let auth = AuthorizationService::new(admins, vec![resolver.clone()]);

    let context = auth.inspect(&message(ChatKind::Group, "7")).await.unwrap();
    assert!(context.bot_admin);
    assert!(context.chat_admin);
    assert_eq!(resolver.calls(), 0);
}

#[tokio::test]
async fn require_chat_manager_accepts_chat_admin_only_in_a_group() {
    let (_store, admins) = service(BTreeMap::new());
    let resolver = FakeResolver::new(PlatformKind::Telegram, ResolverBehavior::Allow);
    let auth = AuthorizationService::new(admins, vec![resolver]);

    let context = auth
        .require_chat_manager(&message(ChatKind::Group, "7"))
        .await
        .unwrap();
    assert!(!context.bot_admin);
    assert!(context.chat_admin);

    let error = auth
        .require_chat_manager(&message(ChatKind::Private, "7"))
        .await
        .unwrap_err();
    assert!(matches!(error, AuthorizationError::Unauthorized));
}

#[tokio::test]
async fn require_bot_admin_rejects_chat_admin_only() {
    let (_store, admins) = service(BTreeMap::new());
    let resolver = FakeResolver::new(PlatformKind::Telegram, ResolverBehavior::Allow);
    let auth = AuthorizationService::new(admins, vec![resolver]);

    let error = auth
        .require_bot_admin(&message(ChatKind::Group, "7"))
        .await
        .unwrap_err();
    assert!(matches!(error, AuthorizationError::Unauthorized));
}
