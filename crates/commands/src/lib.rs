//! Built-in bot commands and their registration.

#![forbid(unsafe_code)]

pub mod admin;
pub mod basic;
pub mod config;
pub mod control;
pub mod pref;
pub mod registry;
pub mod runtime_info;

pub use basic::twenty_four::{
    TwentyFourError, TwentyFourHandle, TwentyFourService, TwentyFourWorker,
};
pub use config::BasicCommandsConfig;
pub use control::ControlCommands;
pub use registry::{CommandBuildError, build_registry, build_registry_with_controls};
pub use runtime_info::RuntimeInfo;
