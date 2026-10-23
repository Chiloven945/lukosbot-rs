//! Minimal Discord adapter built on serenity.

#![forbid(unsafe_code)]

mod adapter;
mod admin;
mod mapping;

pub use adapter::DiscordAdapter;
