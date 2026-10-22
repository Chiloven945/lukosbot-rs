<div align="middle">

# lukosbot-rs

[![CI](https://github.com/Chiloven945/lukosbot-rs/actions/workflows/cargo.yml/badge.svg?branch=master)](https://github.com/Chiloven945/lukosbot-rs/actions/workflows/cargo.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square)](LICENSE)

</div>

A multiplatform chatbot written in Rust, targeting Telegram and Discord.

This repository currently contains the **application foundation**, not the full bot. It provides a
typed domain model, a supervised asynchronous runtime, configuration, logging, SQLite state storage,
a shared HTTP client, and minimal platform adapters. The only end-to-end behaviour is a smoke
`ping` -> `pong` handler used to prove the message pipeline.

## Status

| Area                                                                      | State       |
|---------------------------------------------------------------------------|-------------|
| Cargo workspace and crate boundaries                                      | done        |
| Domain model (messages, addresses, media references)                      | done        |
| Bounded inbound/outbound queues, dispatch, supervision, graceful shutdown | done        |
| Typed configuration, validation, secret redaction                         | done        |
| Structured logging (pretty / compact / JSON)                              | done        |
| SQLite state store and migrations                                         | done        |
| Shared HTTP client with timeouts and response limits                      | done        |
| Telegram / Discord text message adapters                                  | minimal     |
| Command engine, permissions, media pipeline, business features            | not started |

The command engine, policy system, media pipeline, and the migration of the JVM feature set are
separate follow-up plans. See [docs/architecture.md](docs/architecture.md) for the boundaries.

## Layout

```text
crates/
├── domain/               platform-independent types
├── core/                 ports, dispatch, outbound routing, runtime
├── infra/                SQLite state store and shared HTTP client
├── platform-telegram/    teloxide adapter
├── platform-discord/     serenity adapter
└── app/                  CLI, configuration, bootstrap, binary
config/
└── application.example.toml
docs/
├── architecture.md
├── development.md
├── operations.md
└── decisions/            architecture decision records
```

Dependency direction is strictly one-way: `domain <- core <- infra/platforms <- app`.

## Quick start

```bash
# Validate the example configuration without connecting anywhere.
cargo run -p lukosbot-app -- check-config --config config/application.example.toml

# Prepare a local configuration and database.
cp config/application.example.toml config/application.toml
cargo run -p lukosbot-app -- migrate

# Run with credentials supplied through the environment.
LUKOSBOT__PLATFORMS__TELEGRAM__TOKEN='123456:replace-me' \
  LUKOSBOT__PLATFORMS__TELEGRAM__ENABLED=true \
  cargo run -p lukosbot-app -- run
```

Configuration precedence is `CLI > environment > TOML file > defaults`. Nested environment keys use
`__`, for example `LUKOSBOT__RUNTIME__INBOUND_QUEUE_CAPACITY=128`.

## CLI

```text
lukosbot [--config PATH] [--log-filter FILTER] <run|check-config|migrate>
```

- `run` — start the bot and connect enabled platforms.
- `check-config` — load and validate configuration and print a redacted summary.
- `migrate` — apply SQLite migrations and exit.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
```

See [docs/development.md](docs/development.md) for toolchain setup, manual platform testing, and
troubleshooting.

## License

Apache-2.0. See [LICENSE](LICENSE).
