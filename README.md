# lukosbot-rs

[![CI](https://github.com/Chiloven945/lukosbot-rs/actions/workflows/cargo.yml/badge.svg?branch=master)](https://github.com/Chiloven945/lukosbot-rs/actions/workflows/cargo.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square)](LICENSE)

A multiplatform chatbot written in Rust, targeting Telegram and Discord.

The repository currently contains the application foundation, a tested text command engine, basic
commands, persisted state services, bot/chat authorization, policy enforcement, `/admin`, `/pref`,
the third-party API commands (`/github`, `/bilibili`, `/ip`, `/music`, `/whois`), media
send/receive, the Cave, E621, Wikipedia/Minecraft Wiki, Markdown/screenshot and image-help paths,
and an H2 state-import tool. Service scheduling and the remaining JVM business commands remain
follow-up work.

## Status

| Area                                                                         | State       |
|------------------------------------------------------------------------------|-------------|
| Cargo workspace and crate boundaries                                         | done        |
| Domain model (messages, addresses, media references)                         | done        |
| Bounded inbound/outbound queues, dispatch, supervision, graceful shutdown    | done        |
| Typed configuration, validation, secret redaction                            | done        |
| Structured logging (pretty / compact / JSON)                                 | done        |
| SQLite state store, metadata/revision, namespace scans and migrations        | done        |
| Shared HTTP client with timeouts and response limits                         | done        |
| Telegram / Discord text and media adapters                                   | done        |
| Text command engine, aliases, raw/argv parsing, policy-aware text help       | done        |
| Basic commands (`ping`, `echo`, `coin`, `dice`, `luck`, `24`, `help`, ...)   | done        |
| State definitions, resolution service and TTL cleanup worker                 | done        |
| Bootstrap/dynamic bot admins and platform chat-admin resolution              | done        |
| Policy rules, command/help enforcement and allowed-value evaluation          | done        |
| `/admin` and `/pref` command framework                                       | done        |
| Third-party API commands (`/github`, `/bilibili`, `/ip`, `/music`, `/whois`) | done        |
| H2 `bot_state` JSONL import                                                  | done        |
| Media pipeline, Cave, E621, Wiki, screenshots and image help                 | done        |
| Service scheduler, remaining business features                               | not started |

The service scheduler and the remaining JVM commands are separate follow-up plans. See
[docs/architecture.md](docs/architecture.md) and
[docs/command-engine/behavior-matrix.md](docs/command-engine/behavior-matrix.md).

## Layout

```text
crates/
├── domain/               platform-independent types
├── core/                 ports, dispatch, outbound routing, runtime
├── integrations/         external-service contracts and normalized models
├── infra/                SQLite state store, shared HTTP/TCP clients
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
│   ├── third-party-api/  integration behavior matrix
│   └── media-web/        media/web behavior matrix
├── command-engine/
│   └── behavior-matrix.md
└── decisions/            architecture decision records
tools/
└── h2-state-export/      standalone JDBC reference exporter
```

Dependency direction is strictly one-way: `domain <- core <- command-engine <- commands <- app`.
`lukosbot-integrations` defines service contracts and normalized models; `infra` implements them;
platform and infrastructure crates depend on core/domain, never on the command engine.

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
/github user GitHub
/bilibili BV1GJ411x7h7 -i
/ip 1.1.1.1
/music Never Gonna Give You Up
/whois example.com
/cave
/cave add
/e621 search post cat 2
/wiki Java
/wiki md Java
/mcwiki 僵尸猪灵
/mcwiki ss 下界合金
/help wiki img
```

`/luck` also accepts `/l` and `/jrrp`; `/github` accepts `/gh`; `/bilibili` accepts `/bili`.
`/start` is callable but hidden from help. `/weather` is deliberately a placeholder until a weather
integration exists. Discord native slash commands are not implemented yet; these are text commands
on both platforms.

Command registration, prefix, basic/media/web/third-party switches live in the `[commands]`
configuration section. Credentials and provider settings live under `[integrations]`; media, web and
browser limits live under `[media]` and `[web]`. See
[config/application.example.toml](config/application.example.toml),
[third-party behavior matrix](docs/migrations/third-party-api/behavior-matrix.md) and
[media/web behavior matrix](docs/migrations/media-web/behavior-matrix.md).

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
