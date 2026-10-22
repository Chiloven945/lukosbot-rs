//! Binary entry point, configuration loading, dependency assembly, and shutdown coordination.

#![forbid(unsafe_code)]

pub mod bootstrap;
pub mod cli;
pub mod config;
pub mod shutdown;
pub mod telemetry;
