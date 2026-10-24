//! Built-in bot commands and their registration.

#![forbid(unsafe_code)]

pub mod admin;
pub mod basic;
pub mod config;
pub mod control;
pub mod integrations;
pub mod media;
pub mod pref;
pub mod registry;
pub mod rendering;
pub mod runtime_info;
pub mod web;

pub use basic::twenty_four::{
    TwentyFourError, TwentyFourHandle, TwentyFourService, TwentyFourWorker,
};
pub use config::{
    BasicCommandsConfig, MediaCommandsConfig, ThirdPartyCommandsConfig, WebCommandsConfig,
};
pub use control::ControlCommands;
pub use integrations::IntegrationCommandServices;
pub use media::MediaCommandServices;
pub use registry::{
    CommandBuildError, RegistryBuildDependencies, build_registry, build_registry_with_controls,
    build_registry_with_dependencies,
};
pub use runtime_info::RuntimeInfo;
pub use web::WebCommandServices;
