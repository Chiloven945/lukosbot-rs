//! Concrete external-service clients.
//!
//! Provider DTOs stay private to each module; public APIs implement the contracts in
//! `lukosbot-integrations`.

pub mod bilibili;
pub mod e621;
pub mod github;
pub mod ip;
pub mod music;
pub mod whois;
