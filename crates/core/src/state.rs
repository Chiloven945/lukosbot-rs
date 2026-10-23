//! Typed state definitions, registry, resolution service, and expiry maintenance.

pub mod definition;
pub mod error;
pub mod maintenance;
pub mod registry;
pub mod service;

pub use definition::{StateDefinition, StateMetadata, TypedStateDefinition};
pub use error::{StateDefinitionError, StateRegistryError, StateServiceError};
pub use maintenance::{StateMaintenanceError, StateMaintenanceWorker};
pub use registry::{StateRegistry, StateRegistryBuilder};
pub use service::{ResolvedState, StateService};
