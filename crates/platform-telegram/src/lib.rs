//! Minimal Telegram adapter built on teloxide.

#![forbid(unsafe_code)]

mod adapter;
mod admin;
mod mapping;

pub use adapter::{TelegramAdapter, TelegramHttpSettings};
