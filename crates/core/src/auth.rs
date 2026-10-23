//! Bot-administrator and chat-administrator authorization.

pub mod admin;
pub mod authorization;
pub mod context;
pub mod error;
pub mod resolver;

pub use admin::{AdminIdentity, AdminMutation, BotAdminService};
pub use authorization::AuthorizationService;
pub use context::AuthContext;
pub use error::{AdminError, AuthorizationError, ChatAdminLookupError};
pub use resolver::ChatAdminResolver;
