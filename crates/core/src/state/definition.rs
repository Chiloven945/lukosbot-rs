use std::collections::HashSet;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::ports::{StateKey, StateScope, StateScopeKind};

use super::error::StateDefinitionError;

/// Static metadata shared by all state definitions.
#[derive(Debug, Clone)]
pub struct StateMetadata {
    pub name: String,
    pub namespace: String,
    pub description: String,
    pub allowed_scopes: Vec<StateScopeKind>,
    pub preferred_scope: StateScopeKind,
    pub resolve_order: Vec<StateScopeKind>,
    pub suggestions: Vec<String>,
    pub ttl: Option<Duration>,
}

impl StateMetadata {
    pub fn new(
        name: impl Into<String>,
        namespace: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            namespace: namespace.into(),
            description: description.into(),
            allowed_scopes: vec![
                StateScopeKind::Global,
                StateScopeKind::Chat,
                StateScopeKind::User,
            ],
            preferred_scope: StateScopeKind::User,
            resolve_order: vec![
                StateScopeKind::Chat,
                StateScopeKind::User,
                StateScopeKind::Global,
            ],
            suggestions: Vec::new(),
            ttl: None,
        }
    }

    pub fn allowed_scopes(mut self, scopes: impl IntoIterator<Item = StateScopeKind>) -> Self {
        self.allowed_scopes = scopes.into_iter().collect();
        self
    }

    pub fn preferred_scope(mut self, scope: StateScopeKind) -> Self {
        self.preferred_scope = scope;
        self
    }

    pub fn resolve_order(mut self, scopes: impl IntoIterator<Item = StateScopeKind>) -> Self {
        self.resolve_order = scopes.into_iter().collect();
        self
    }

    pub fn suggestions(mut self, suggestions: impl IntoIterator<Item = String>) -> Self {
        self.suggestions = suggestions.into_iter().collect();
        self
    }

    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    pub(crate) fn validate(&self) -> Result<(), StateDefinitionError> {
        StateKey::new(
            StateScope::Global,
            self.namespace.clone(),
            self.name.clone(),
        )
        .map_err(|error| StateDefinitionError::invalid(error.to_string()))?;

        if self.name.chars().any(char::is_whitespace) {
            return Err(StateDefinitionError::invalid(
                "name must not contain whitespace",
            ));
        }

        validate_scope_list("allowed_scopes", &self.allowed_scopes)?;
        if !self.allowed_scopes.contains(&self.preferred_scope) {
            return Err(StateDefinitionError::invalid(
                "preferred_scope must be included in allowed_scopes",
            ));
        }

        validate_scope_list("resolve_order", &self.resolve_order)?;
        for scope in &self.resolve_order {
            if !self.allowed_scopes.contains(scope) {
                return Err(StateDefinitionError::invalid(format!(
                    "resolve_order contains disallowed scope `{scope}`"
                )));
            }
        }

        if self.ttl.is_some_and(|ttl| ttl.is_zero()) {
            return Err(StateDefinitionError::invalid(
                "ttl must be greater than zero",
            ));
        }

        Ok(())
    }
}

fn validate_scope_list(label: &str, scopes: &[StateScopeKind]) -> Result<(), StateDefinitionError> {
    if scopes.is_empty() {
        return Err(StateDefinitionError::invalid(format!(
            "{label} must not be empty"
        )));
    }

    let mut seen = HashSet::new();
    for scope in scopes {
        if !seen.insert(*scope) {
            return Err(StateDefinitionError::invalid(format!(
                "{label} contains duplicate scope `{scope}`"
            )));
        }
    }
    Ok(())
}

/// An object-safe state definition.
///
/// Typed implementations keep their native type behind JSON at this boundary; the registry never
/// needs `Any` or runtime downcasts.
pub trait StateDefinition: Send + Sync {
    fn name(&self) -> &str;
    fn namespace(&self) -> &str;
    fn description(&self) -> &str;

    fn allowed_scopes(&self) -> &[StateScopeKind];
    fn preferred_scope(&self) -> StateScopeKind;
    fn resolve_order(&self) -> &[StateScopeKind];

    fn default_value(&self) -> Result<Value, StateDefinitionError>;
    fn parse_value(&self, raw: &str) -> Result<Value, StateDefinitionError>;
    fn validate_value(&self, value: &Value) -> Result<(), StateDefinitionError>;
    fn format_value(&self, value: &Value) -> Result<String, StateDefinitionError>;

    fn suggestions(&self) -> &[String];
    fn ttl(&self) -> Option<Duration>;
}

type ParseFn<T> = Box<dyn Fn(&str) -> Result<T, StateDefinitionError> + Send + Sync>;
type ValidateFn<T> = Box<dyn Fn(&T) -> Result<(), StateDefinitionError> + Send + Sync>;
type FormatFn<T> = Box<dyn Fn(&T) -> String + Send + Sync>;

/// A typed definition backed by a parser, an optional validator, and a formatter.
pub struct TypedStateDefinition<T> {
    metadata: StateMetadata,
    default: T,
    parse: ParseFn<T>,
    validate: Option<ValidateFn<T>>,
    format: FormatFn<T>,
}

impl<T> TypedStateDefinition<T>
where
    T: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    pub fn new(
        metadata: StateMetadata,
        default: T,
        parse: impl Fn(&str) -> Result<T, StateDefinitionError> + Send + Sync + 'static,
        format: impl Fn(&T) -> String + Send + Sync + 'static,
    ) -> Self {
        Self {
            metadata,
            default,
            parse: Box::new(parse),
            validate: None,
            format: Box::new(format),
        }
    }

    pub fn with_validator(
        mut self,
        validate: impl Fn(&T) -> Result<(), StateDefinitionError> + Send + Sync + 'static,
    ) -> Self {
        self.validate = Some(Box::new(validate));
        self
    }

    pub fn metadata(&self) -> &StateMetadata {
        &self.metadata
    }
}

impl<T> StateDefinition for TypedStateDefinition<T>
where
    T: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn namespace(&self) -> &str {
        &self.metadata.namespace
    }

    fn description(&self) -> &str {
        &self.metadata.description
    }

    fn allowed_scopes(&self) -> &[StateScopeKind] {
        &self.metadata.allowed_scopes
    }

    fn preferred_scope(&self) -> StateScopeKind {
        self.metadata.preferred_scope
    }

    fn resolve_order(&self) -> &[StateScopeKind] {
        &self.metadata.resolve_order
    }

    fn default_value(&self) -> Result<Value, StateDefinitionError> {
        serde_json::to_value(&self.default).map_err(Into::into)
    }

    fn parse_value(&self, raw: &str) -> Result<Value, StateDefinitionError> {
        let value = (self.parse)(raw)?;
        if let Some(validate) = &self.validate {
            validate(&value)?;
        }
        serde_json::to_value(value).map_err(Into::into)
    }

    fn validate_value(&self, value: &Value) -> Result<(), StateDefinitionError> {
        let typed: T = serde_json::from_value(value.clone())?;
        if let Some(validate) = &self.validate {
            validate(&typed)?;
        }
        Ok(())
    }

    fn format_value(&self, value: &Value) -> Result<String, StateDefinitionError> {
        let typed: T = serde_json::from_value(value.clone())?;
        Ok((self.format)(&typed))
    }

    fn suggestions(&self) -> &[String] {
        &self.metadata.suggestions
    }

    fn ttl(&self) -> Option<Duration> {
        self.metadata.ttl
    }
}
