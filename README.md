# lukosbot-rs

<div align="middle">

[![CI](https://github.com/Chiloven945/lukosbot-rs/actions/workflows/cargo.yml/badge.svg?branch=master)](https://github.com/Chiloven945/lukosbot-rs/actions/workflows/cargo.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square)](LICENSE)

</div>

A multiplatform chatbot written in Rust, targeting Telegram and Discord.

The repository currently contains the application foundation, a tested text command engine, basic
commands, persisted state services, bot/chat authorization, policy enforcement, `/admin`, `/pref`,
and an H2 state-import tool. Media, service scheduling, and most JVM business commands remain
follow-up work.

## Status

| Area                                                                       | State       |
|----------------------------------------------------------------------------|-------------|
| Cargo workspace and crate boundaries                                       | done        |
| Domain model (messages, addresses, media references)                       | done        |
| Bounded inbound/outbound queues, dispatch, supervision, graceful shutdown  | done        |
| Typed configuration, validation, secret redaction                          | done        |
| Structured logging (pretty / compact / JSON)                               | done        |
| SQLite state store, metadata/revision, namespace scans and migrations      | done        |
| Shared HTTP client with timeouts and response limits                       | done        |
| Telegram / Discord text message adapters                                   | minimal     |
| Text command engine, aliases, raw/argv parsing, policy-aware text help     | done        |
| Basic commands (`ping`, `echo`, `coin`, `dice`, `luck`, `24`, `help`, ...) | done        |
| State definitions, resolution service and TTL cleanup worker               | done        |
| Bootstrap/dynamic bot admins and platform chat-admin resolution            | done        |
| Policy rules, command/help enforcement and allowed-value evaluation        | done        |
| `/admin` and `/pref` command framework                                     | done        |
| H2 `bot_state` JSONL import                                                | done        |
| Media pipeline, service scheduler, integrations, business features         | not started |

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
├── migrations/           H2 JSONL export/import contract
├── command-engine/
│   └── behavior-matrix.md
└── decisions/            architecture decision records
tools/
└── h2-state-export/      standalone JDBC reference exporter
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
/admin me
/admin list
/pref list
/help
```

`/luck` also accepts `/l` and `/jrrp`. `/start` is callable but hidden from help. `/weather` is
deliberately a placeholder until a weather integration exists. Discord native slash commands are not
implemented yet; these are text commands on both platforms.

Command registration, prefix, and basic-command switches live in the `[commands]` configuration
section. See [config/application.example.toml](config/application.example.toml).

## CLI

```text
lukosbot [--config PATH] [--log-filter FILTER] [run|check-config|migrate|import-state]
```

- `run` — start the bot and connect enabled platforms. This is also the default when no subcommand
  is supplied, so `cargo run` and `lukosbot` start the bot directly.
- `check-config` — load and validate configuration and print a redacted summary.
- `migrate` — apply SQLite migrations and exit.
- `import-state --input PATH [--apply] [--on-conflict fail|skip|replace]` — validate or import a JVM
  `bot_state` JSONL export. It is a dry-run unless `--apply` is supplied.

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
