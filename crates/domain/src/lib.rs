//! Platform-independent domain types shared by the core runtime and platform adapters.
//!
//! This crate deliberately depends on no platform SDK, database driver, or HTTP client. It is
//! the common language between adapters and application logic.
//!
//! Identifiers are newtypes with validated constructors, so adapters cannot fabricate
//! syntactically invalid platform IDs. Message bodies are intentionally not serializable: the
//! Rust struct layout is not a persistence or wire protocol.

#![forbid(unsafe_code)]

mod address;
mod media;
mod message;
mod platform;

pub use address::{ChatAddress, InvalidPlatformId, PlatformChatId, PlatformUserId, Sender};
pub use media::{MediaRef, MediaSource};
pub use message::{
    InboundMessage, InboundPart, MessageContext, OutboundMessage, OutboundPart, PlatformMessageId,
    SentMessage,
};
pub use platform::{ChatKind, ParsePlatformKindError, PlatformKind};
