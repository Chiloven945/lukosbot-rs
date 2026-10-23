use std::collections::BTreeMap;
use std::sync::Arc;

use super::definition::StateDefinition;
use super::error::StateRegistryError;

/// An immutable, name-indexed collection of state definitions.
#[derive(Clone, Default)]
pub struct StateRegistry {
    definitions: BTreeMap<String, Arc<dyn StateDefinition>>,
}

impl StateRegistry {
    pub fn new(
        definitions: impl IntoIterator<Item = Arc<dyn StateDefinition>>,
    ) -> Result<Self, StateRegistryError> {
        let mut builder = StateRegistryBuilder::new();
        for definition in definitions {
            builder.register(definition)?;
        }
        Ok(builder.build())
    }

    pub fn find(&self, name: &str) -> Option<&dyn StateDefinition> {
        self.definitions.get(name).map(|definition| &**definition)
    }

    pub fn all(&self) -> impl Iterator<Item = &dyn StateDefinition> {
        self.definitions.values().map(|definition| &**definition)
    }

    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    pub fn len(&self) -> usize {
        self.definitions.len()
    }
}

/// Builds a [`StateRegistry`] while rejecting duplicate or malformed definitions.
#[derive(Default)]
pub struct StateRegistryBuilder {
    definitions: BTreeMap<String, Arc<dyn StateDefinition>>,
}

impl StateRegistryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        definition: Arc<dyn StateDefinition>,
    ) -> Result<(), StateRegistryError> {
        let name = definition.name().trim().to_owned();
        let display_name = if name.is_empty() {
            "<unnamed>".to_owned()
        } else {
            name.clone()
        };

        if self.definitions.contains_key(&name) && !name.is_empty() {
            return Err(StateRegistryError::DuplicateName { name });
        }

        validate_definition(definition.as_ref()).map_err(|message| {
            StateRegistryError::InvalidDefinition {
                name: display_name,
                message,
            }
        })?;

        self.definitions.insert(name, definition);
        Ok(())
    }

    pub fn build(self) -> StateRegistry {
        StateRegistry {
            definitions: self.definitions,
        }
    }
}

fn validate_definition(definition: &dyn StateDefinition) -> Result<(), String> {
    let metadata = StateMetadataView {
        name: definition.name(),
        namespace: definition.namespace(),
        allowed_scopes: definition.allowed_scopes(),
        preferred_scope: definition.preferred_scope(),
        resolve_order: definition.resolve_order(),
        ttl: definition.ttl(),
    };
    metadata.validate()?;

    let default = definition
        .default_value()
        .map_err(|error| error.to_string())?;
    definition
        .validate_value(&default)
        .map_err(|error| format!("default value is invalid: {error}"))?;
    definition
        .format_value(&default)
        .map_err(|error| format!("default value cannot be formatted: {error}"))?;

    Ok(())
}

struct StateMetadataView<'a> {
    name: &'a str,
    namespace: &'a str,
    allowed_scopes: &'a [crate::ports::StateScopeKind],
    preferred_scope: crate::ports::StateScopeKind,
    resolve_order: &'a [crate::ports::StateScopeKind],
    ttl: Option<std::time::Duration>,
}

impl StateMetadataView<'_> {
    fn validate(&self) -> Result<(), String> {
        use super::definition::StateMetadata;

        StateMetadata {
            name: self.name.to_owned(),
            namespace: self.namespace.to_owned(),
            description: String::new(),
            allowed_scopes: self.allowed_scopes.to_vec(),
            preferred_scope: self.preferred_scope,
            resolve_order: self.resolve_order.to_vec(),
            suggestions: Vec::new(),
            ttl: self.ttl,
        }
        .validate()
        .map_err(|error| error.to_string())
    }
}
