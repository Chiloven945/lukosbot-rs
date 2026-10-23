pub mod platform;
pub mod state_store;

pub use platform::{InboundSender, PlatformAdapter, PlatformError};
pub use state_store::{
    ParseStateScopeKindError, SCHEMA_VERSION, StateEntry, StateKey, StateScope, StateScopeKind,
    StateStore, StateStoreError, VersionedValue,
};
