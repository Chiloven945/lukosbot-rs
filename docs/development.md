# Development

## Toolchain

The repository pins Rust `1.99.1` in `rust-toolchain.toml`. The declared MSRV is `1.94`, driven by
SQLx 0.9, and is verified by a dedicated CI job.

```bash
rustup show          # confirms the pinned toolchain
rustc --version
```

SQLite is bundled through `sqlx`/`libsqlite3-sys`, and the Discord gateway uses native TLS. On
Debian or Ubuntu:

```bash
sudo apt install build-essential pkg-config libssl-dev
```

## Local configuration

```bash
cp config/application.example.toml config/application.toml
cp .env.example .env
```

`config/application.toml`, `.env`, and `data/` are ignored by git. Prefer environment variables for
credentials; the TOML file is the right place for non-secret settings.

```bash
# Inspect the fully resolved, redacted configuration.
cargo run -p lukosbot-app -- check-config

# Apply or re-apply schema migrations.
cargo run -p lukosbot-app -- migrate
```

## Checks

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features

# Optional platform builds
cargo check --workspace --no-default-features --all-targets
cargo test --workspace --all-targets --all-features
```

Core runtime tests require the `test-util` feature:

```bash
cargo test -p lukosbot-core --features test-util
```

## Test strategy

| Layer                           | Location                                              | External services |
|---------------------------------|-------------------------------------------------------|-------------------|
| Domain                          | inline unit tests                                     | no                |
| Configuration / CLI / telemetry | `crates/app`                                          | no                |
| Runtime lifecycle               | `crates/core/tests/runtime.rs` with `test-util` fakes | no                |
| SQLite                          | `crates/infra/tests/storage.rs` with temp files       | no                |
| HTTP                            | `crates/infra/tests/http.rs` with `wiremock`          | no                |
| Platform mapping                | JSON fixtures in each adapter                         | no                |
| Command engine                  | `crates/command-engine/tests/engine.rs`               | no                |
| Built-in commands / 24 actor    | `crates/commands` inline and e2e tests                | no                |
| Manual end-to-end               | dedicated test bot                                    | yes               |

Never commit real tokens or production data. Do not run a test bot against the same token as a
production instance: Telegram permits only one long-polling consumer per token.

## Manual Telegram test

1. Create a dedicated bot with BotFather and enable the token in your local environment:
   ```bash
   export LUKOSBOT__PLATFORMS__TELEGRAM__ENABLED=true
   export LUKOSBOT__PLATFORMS__TELEGRAM__TOKEN='123456:test-token'
   ```
2. Start the bot: `cargo run -p lukosbot-app -- run`.
3. Send these prefix commands in a private chat:
   ```text
   /ping
   /echo hello world
   /dice 3
   /luck
   /help
   /24
   ```
   Bare `ping` is intentionally ignored. `/start` is callable but hidden from help. `/weather`
   reports that the integration is not available yet.
4. For `/24`, submit an expression, use `/24 giveup`, or wait for the configured timeout.
5. Press Ctrl-C; the process should exit within `app.shutdown_timeout_secs`.

## Manual Discord test

1. Create a test application and bot, enable the **Message Content Intent** in the developer portal.
2. Invite the bot to a test guild with permission to read and send messages.
3. ```bash
   export LUKOSBOT__PLATFORMS__DISCORD__ENABLED=true
   export LUKOSBOT__PLATFORMS__DISCORD__TOKEN='test-token'
   cargo run -p lukosbot-app -- run
   ```
4. Send `/ping`, `/echo hello world`, `/dice 3`, and `/help` in a channel the bot can read. These
   are text prefix commands, not native Discord slash commands.
5. Confirm ordinary chat text is ignored, then stop with Ctrl-C and confirm a clean exit.

## Telegram troubleshooting

Telegram uses long polling with a 30-second server-side timeout. The teloxide HTTP client must have
a longer timeout, otherwise every otherwise-correct long poll is aborted by the client and the
adapter backs off repeatedly. The adapter enforces a client timeout of at least 45 seconds
regardless of the shared `http.request_timeout_secs` setting.

`http.proxy` is applied to the Telegram HTTP client. SOCKS proxies such as
`socks5h://127.0.0.1:10808` are supported. The proxy URL is not logged.

## Debugging

- `--log-filter debug` (or `RUST_LOG=debug`) shows adapter polling, dispatch, and send attempts.
- `check-config` does not contact any platform, so it is safe to run in CI-like environments.
- Logs never include tokens, full message bodies, or full request URLs.
- If the database cannot be opened, startup fails before any platform connection is attempted.
