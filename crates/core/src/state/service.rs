use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::warn;

use lukosbot_domain::{ChatAddress, PlatformUserId};

use crate::ports::{
    StateKey, StateScope, StateScopeKind, StateStore, StateStoreError, VersionedValue,
};

use super::definition::StateDefinition;
use super::error::{StateDefinitionError, StateServiceError};
use super::registry::StateRegistry;

/// The effective value and the scope it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedState {
    pub value: Value,
    pub source_scope: Option<StateScopeKind>,
}

/// High-level state access built on [`StateStore`].
#[derive(Clone)]
pub struct StateService {
    store: Arc<dyn StateStore>,
    registry: Arc<StateRegistry>,
}

impl StateService {
    pub fn new(store: Arc<dyn StateStore>, registry: Arc<StateRegistry>) -> Self {
        Self { store, registry }
    }

    pub fn registry(&self) -> &Arc<StateRegistry> {
        &self.registry
    }

    pub async fn resolve(
        &self,
        definition: &dyn StateDefinition,
        address: &ChatAddress,
        user_id: Option<&PlatformUserId>,
    ) -> Result<ResolvedState, StateServiceError> {
        for kind in definition.resolve_order() {
            let Some(scope) = scope_for(*kind, address, user_id) else {
                continue;
            };

            match self.get_at_scope(definition, &scope).await {
                Ok(Some(value)) => {
                    return Ok(ResolvedState {
                        value: value.value,
                        source_scope: Some(*kind),
                    });
                }
                Ok(None) => {}
                Err(StateServiceError::InvalidStoredValue { state, scope }) => {
                    warn!(
                        state = %state,
                        scope = %scope,
                        "ignoring invalid stored state value; falling back"
                    );
                }
                Err(error) => return Err(error),
            }
        }

        let value = definition
            .default_value()
            .map_err(|source| StateServiceError::Definition {
                state: definition.name().to_owned(),
                source,
            })?;
        definition
            .validate_value(&value)
            .map_err(|source| StateServiceError::Definition {
                state: definition.name().to_owned(),
                source,
            })?;

        Ok(ResolvedState {
            value,
            source_scope: None,
        })
    }

    pub async fn get_at_scope(
        &self,
        definition: &dyn StateDefinition,
        scope: &StateScope,
    ) -> Result<Option<VersionedValue>, StateServiceError> {
        ensure_allowed(definition, scope.kind())?;
        let key = key_for(definition, scope)?;

        let value = match self.store.get(&key).await {
            Ok(value) => value,
            Err(StateStoreError::Codec(_)) => {
                return Err(StateServiceError::InvalidStoredValue {
                    state: definition.name().to_owned(),
                    scope: scope.kind(),
                });
            }
            Err(error) => return Err(error.into()),
        };

        let Some(value) = value else {
            return Ok(None);
        };

        definition.validate_value(&value.value).map_err(|_| {
            StateServiceError::InvalidStoredValue {
                state: definition.name().to_owned(),
                scope: scope.kind(),
            }
        })?;

        Ok(Some(value))
    }

    pub async fn set_at_scope(
        &self,
        definition: &dyn StateDefinition,
        scope: &StateScope,
        raw_value: &str,
    ) -> Result<Value, StateServiceError> {
        ensure_allowed(definition, scope.kind())?;
        let key = key_for(definition, scope)?;

        let value =
            definition
                .parse_value(raw_value)
                .map_err(|source| StateServiceError::Parse {
                    state: definition.name().to_owned(),
                    source,
                })?;
        definition
            .validate_value(&value)
            .map_err(|source| StateServiceError::Validation {
                state: definition.name().to_owned(),
                source,
            })?;

        let expires_at = ttl_expiry(definition)?;
        self.store.put(key, value.clone(), expires_at).await?;
        Ok(value)
    }

    pub async fn clear_at_scope(
        &self,
        definition: &dyn StateDefinition,
        scope: &StateScope,
    ) -> Result<(), StateServiceError> {
        ensure_allowed(definition, scope.kind())?;
        let key = key_for(definition, scope)?;
        self.store.delete(&key).await?;
        Ok(())
    }
}

fn ensure_allowed(
    definition: &dyn StateDefinition,
    scope: StateScopeKind,
) -> Result<(), StateServiceError> {
    if definition.allowed_scopes().contains(&scope) {
        Ok(())
    } else {
        Err(StateServiceError::UnsupportedScope {
            state: definition.name().to_owned(),
            scope,
        })
    }
}

fn key_for(
    definition: &dyn StateDefinition,
    scope: &StateScope,
) -> Result<StateKey, StateServiceError> {
    StateKey::new(scope.clone(), definition.namespace(), definition.name()).map_err(|error| {
        StateServiceError::Definition {
            state: definition.name().to_owned(),
            source: StateDefinitionError::invalid(error.to_string()),
        }
    })
}

fn ttl_expiry(
    definition: &dyn StateDefinition,
) -> Result<Option<DateTime<Utc>>, StateServiceError> {
    let Some(ttl) = definition.ttl() else {
        return Ok(None);
    };
    let ttl = chrono::Duration::from_std(ttl).map_err(|error| StateServiceError::Definition {
        state: definition.name().to_owned(),
        source: StateDefinitionError::invalid(format!("invalid ttl: {error}")),
    })?;
    Ok(Some(Utc::now() + ttl))
}

fn scope_for(
    kind: StateScopeKind,
    address: &ChatAddress,
    user_id: Option<&PlatformUserId>,
) -> Option<StateScope> {
    match kind {
        StateScopeKind::Global => Some(StateScope::Global),
        StateScopeKind::Chat => Some(StateScope::chat(address)),
        StateScopeKind::User => user_id
            .cloned()
            .map(|user_id| StateScope::user(address.platform, user_id)),
    }
}
