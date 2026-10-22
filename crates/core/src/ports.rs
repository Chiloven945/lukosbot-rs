pub mod platform;
pub mod state_store;

pub use platform::{InboundSender, PlatformAdapter, PlatformError};
pub use state_store::{
    StateKey, StateScope, StateStore, StateStoreError, VALUE_VERSION, VersionedValue,
};
