//! Built-in bot commands and their registration.

#![forbid(unsafe_code)]

pub mod basic;
pub mod config;
pub mod registry;
pub mod runtime_info;

pub use basic::twenty_four::{
    TwentyFourError, TwentyFourHandle, TwentyFourService, TwentyFourWorker,
};
pub use config::BasicCommandsConfig;
pub use registry::build_registry;
pub use runtime_info::RuntimeInfo;
