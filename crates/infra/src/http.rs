//! Shared HTTP client with explicit timeouts, proxy support, and response size limits.

mod client;

pub use client::{HttpClient, HttpError, HttpSettings, RetryPolicy};
