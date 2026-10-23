# lukosbot-rs

<div align="middle">

[![CI](https://github.com/Chiloven945/lukosbot-rs/actions/workflows/cargo.yml/badge.svg?branch=master)](https://github.com/Chiloven945/lukosbot-rs/actions/workflows/cargo.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square)](LICENSE)

</div>

A multiplatform chatbot written in Rust, targeting Telegram and Discord.

The repository currently contains the application foundation plus a tested text command engine and
the first set of basic commands. It does not yet include the policy system, media pipeline, or the
full JVM feature set.

## Status

| Area                                                                       | State       |
|----------------------------------------------------------------------------|-------------|
| Cargo workspace and crate boundaries                                       | done        |
| Domain model (messages, addresses, media references)                       | done        |
| Bounded inbound/outbound queues, dispatch, supervision, graceful shutdown  | done        |
| Typed configuration, validation, secret redaction                          | done        |
| Structured logging (pretty / compact / JSON)                               | done        |
| SQLite state store and migrations                                          | done        |
| Shared HTTP client with timeouts and response limits                       | done        |
| Telegram / Discord text message adapters                                   | minimal     |
| Text command engine, aliases, raw/argv parsing, text help                  | done        |
| Basic commands (`ping`, `echo`, `coin`, `dice`, `luck`, `24`, `help`, ...) | done        |
| Permissions, media pipeline, integrations, business features               | not started |

The policy system, media pipeline, and the remaining JVM commands are separate follow-up plans. See
[docs/architecture.md](docs/architecture.md) and
[docs/command-engine/behavior-matrix.md](docs/command-engine/behavior-matrix.md).

## Layout

```text
crates/
├── domain/               platform-independent types
├── core/                 ports, dispatch, outbound routing, runtime
├── infra/                SQLite state store and shared HTTP client
├── command-engine/       command definitions, parsing, registry, help
├── commands/             built-in commands and the 24-point actor
├── platform-telegram/    teloxide adapter
├── platform-discord/     serenity adapter
└── app/                  CLI, configuration, bootstrap, binary
config/
└── application.example.toml
docs/
├── architecture.md
├── development.md
├── operations.md
├── command-engine/
│   └── behavior-matrix.md
└── decisions/            architecture decision records
```

Dependency direction is strictly one-way: `domain <- core <- command-engine <- commands <- app`.
Platform and infrastructure crates depend on core/domain, never on the command engine.

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

## Bot commands

The default prefix is `/`. Type a command as a normal text message:

```text
/ping
/echo hello world
/coin 10
/dice 3
/luck
/24
/help
```

`/luck` also accepts `/l` and `/jrrp`. `/start` is callable but hidden from help. `/weather` is
deliberately a placeholder until a weather integration exists. Discord native slash commands are not
implemented yet; these are text commands on both platforms.

Command registration, prefix, and basic-command switches live in the `[commands]` configuration
section. See [config/application.example.toml](config/application.example.toml).

## CLI

```text
lukosbot [--config PATH] [--log-filter FILTER] [run|check-config|migrate]
```

- `run` — start the bot and connect enabled platforms. This is also the default when no subcommand
  is supplied, so `cargo run` and `lukosbot` start the bot directly.
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
