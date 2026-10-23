use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;

use lukosbot_core::StateStore;
use lukosbot_core::ports::{StateKey, StateScope, StateScopeKind};
use lukosbot_core::state::{
    StateDefinition, StateDefinitionError, StateMetadata, StateRegistry, StateService,
    StateServiceError, TypedStateDefinition,
};
use lukosbot_core::testing::MemoryStateStore;
use lukosbot_domain::{ChatAddress, ChatKind, PlatformChatId, PlatformKind, PlatformUserId};

fn address(kind: ChatKind) -> ChatAddress {
    ChatAddress::new(
        PlatformKind::Telegram,
        PlatformChatId::new("100").unwrap(),
        kind,
    )
}

fn user_id() -> PlatformUserId {
    PlatformUserId::new("7").unwrap()
}

fn definition() -> Arc<dyn StateDefinition> {
    let metadata = StateMetadata::new("lang", "prefs", "回复语言")
        .allowed_scopes([
            StateScopeKind::Global,
            StateScopeKind::Chat,
            StateScopeKind::User,
        ])
        .preferred_scope(StateScopeKind::User)
        .resolve_order([
            StateScopeKind::Chat,
            StateScopeKind::User,
            StateScopeKind::Global,
        ])
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

fn service(def: Arc<dyn StateDefinition>) -> (Arc<dyn StateStore>, Arc<StateService>) {
    let store: Arc<dyn StateStore> = Arc::new(MemoryStateStore::new());
    let registry = Arc::new(StateRegistry::new(vec![def]).unwrap());
    let service = Arc::new(StateService::new(Arc::clone(&store), registry));
    (store, service)
}

#[tokio::test]
async fn resolves_in_definition_order_and_falls_back_to_default() {
    let def = definition();
    let (store, service) = service(Arc::clone(&def));
    let address = address(ChatKind::Group);
    let user = user_id();

    let resolved = service
        .resolve(def.as_ref(), &address, Some(&user))
        .await
        .unwrap();
    assert_eq!(resolved.value, json!("zh"));
    assert_eq!(resolved.source_scope, None);

    service
        .set_at_scope(def.as_ref(), &StateScope::Global, "global")
        .await
        .unwrap();
    service
        .set_at_scope(
            def.as_ref(),
            &StateScope::user(PlatformKind::Telegram, user.clone()),
            "user",
        )
        .await
        .unwrap();
    service
        .set_at_scope(def.as_ref(), &StateScope::chat(&address), "chat")
        .await
        .unwrap();

    let resolved = service
        .resolve(def.as_ref(), &address, Some(&user))
        .await
        .unwrap();
    assert_eq!(resolved.value, json!("chat"));
    assert_eq!(resolved.source_scope, Some(StateScopeKind::Chat));

    service
        .clear_at_scope(def.as_ref(), &StateScope::chat(&address))
        .await
        .unwrap();
    let resolved = service
        .resolve(def.as_ref(), &address, Some(&user))
        .await
        .unwrap();
    assert_eq!(resolved.value, json!("user"));
    assert_eq!(resolved.source_scope, Some(StateScopeKind::User));

    let direct = service
        .get_at_scope(
            def.as_ref(),
            &StateScope::user(PlatformKind::Telegram, user),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(direct.value, json!("user"));
    assert_eq!(direct.revision, 0);

    assert!(
        store
            .get(&StateKey::global("prefs", "lang").unwrap())
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn missing_user_scope_is_skipped() {
    let def = definition();
    let (_store, service) = service(Arc::clone(&def));
    let address = address(ChatKind::Group);

    service
        .set_at_scope(
            def.as_ref(),
            &StateScope::user(PlatformKind::Telegram, user_id()),
            "user",
        )
        .await
        .unwrap();

    let resolved = service.resolve(def.as_ref(), &address, None).await.unwrap();
    assert_eq!(resolved.value, json!("zh"));
}

#[tokio::test]
async fn invalid_stored_value_falls_back_but_is_reported_explicitly() {
    let def = definition();
    let (store, service) = service(Arc::clone(&def));
    let address = address(ChatKind::Group);
    let key = StateKey::new(StateScope::chat(&address), "prefs", "lang").unwrap();
    store.put(key.clone(), json!(42), None).await.unwrap();

    let resolved = service
        .resolve(def.as_ref(), &address, Some(&user_id()))
        .await
        .unwrap();
    assert_eq!(resolved.value, json!("zh"));

    let error = service
        .get_at_scope(def.as_ref(), &StateScope::chat(&address))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StateServiceError::InvalidStoredValue {
            scope: StateScopeKind::Chat,
            ..
        }
    ));
}

#[tokio::test]
async fn expired_values_fall_back_to_a_lower_scope() {
    let def = definition();
    let (store, service) = service(Arc::clone(&def));
    let address = address(ChatKind::Group);
    let key = StateKey::new(StateScope::chat(&address), "prefs", "lang").unwrap();

    store
        .put(
            key,
            json!("expired"),
            Some(Utc::now() - ChronoDuration::seconds(1)),
        )
        .await
        .unwrap();
    service
        .set_at_scope(def.as_ref(), &StateScope::Global, "global")
        .await
        .unwrap();

    let resolved = service
        .resolve(def.as_ref(), &address, Some(&user_id()))
        .await
        .unwrap();
    assert_eq!(resolved.value, json!("global"));
    assert_eq!(resolved.source_scope, Some(StateScopeKind::Global));
}

#[tokio::test]
async fn ttl_sets_an_expiry_and_unsupported_scope_is_rejected() {
    let metadata = StateMetadata::new("status", "prefs", "状态")
        .allowed_scopes([StateScopeKind::User])
        .preferred_scope(StateScopeKind::User)
        .resolve_order([StateScopeKind::User])
        .ttl(Duration::from_secs(60));
    let def: Arc<dyn StateDefinition> = Arc::new(TypedStateDefinition::new(
        metadata,
        "idle".to_owned(),
        |raw| Ok(raw.trim().to_owned()),
        |value| value.clone(),
    ));
    let (_store, service) = service(def.clone());

    let value = service
        .set_at_scope(
            def.as_ref(),
            &StateScope::user(PlatformKind::Telegram, user_id()),
            "busy",
        )
        .await
        .unwrap();
    assert_eq!(value, json!("busy"));

    let stored = service
        .get_at_scope(
            def.as_ref(),
            &StateScope::user(PlatformKind::Telegram, user_id()),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(stored.expires_at.is_some());

    let error = service
        .set_at_scope(def.as_ref(), &StateScope::Global, "busy")
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StateServiceError::UnsupportedScope {
            scope: StateScopeKind::Global,
            ..
        }
    ));
}

#[test]
fn registry_rejects_duplicate_names_and_invalid_metadata() {
    let def = definition();
    assert!(StateRegistry::new(vec![Arc::clone(&def), def]).is_err());

    let metadata = StateMetadata::new("broken", "prefs", "broken")
        .allowed_scopes([StateScopeKind::User])
        .preferred_scope(StateScopeKind::Chat)
        .resolve_order([StateScopeKind::User]);
    let broken: Arc<dyn StateDefinition> = Arc::new(TypedStateDefinition::new(
        metadata,
        "value".to_owned(),
        |raw| Ok(raw.to_owned()),
        |value| value.clone(),
    ));
    assert!(StateRegistry::new(vec![broken]).is_err());
}
