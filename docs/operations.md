# Operations

## Process interface

```text
lukosbot [--config PATH] [--log-filter FILTER] [run|check-config|migrate|import-state]
```

Omitting the subcommand is equivalent to `run`.

Exit codes:

| Code | Meaning                                               |
|------|-------------------------------------------------------|
| 0    | Normal completion or clean shutdown                   |
| 1    | Configuration, startup, runtime, or migration failure |
| 2    | CLI usage error (from `clap`)                         |

`run` exits non-zero if a supervised task fails (for example, an adapter stops unexpectedly or a
task panics), even though shutdown itself still completes in an orderly fashion.

## Configuration sources

Priority: `CLI > environment > TOML file > defaults`.

Environment keys use the `LUKOSBOT` prefix and `__` between levels:

```bash
LUKOSBOT__APP__ENVIRONMENT=production
LUKOSBOT__LOGGING__FORMAT=json
LUKOSBOT__PLATFORMS__TELEGRAM__TOKEN=...
LUKOSBOT__STORAGE__SQLITE_PATH=/var/lib/lukosbot/state.sqlite3
LUKOSBOT__COMMANDS__PREFIX=/
LUKOSBOT__COMMANDS__BASIC__DICE=false
LUKOSBOT__COMMANDS__TWENTY_FOUR__TIME_LIMIT_SECS=300
LUKOSBOT__COMMANDS__CONTROL__ADMIN=true
LUKOSBOT__STORAGE__EXPIRY_CLEANUP_INTERVAL_SECS=3600
```

Bootstrap bot administrators and policy rules are structured TOML configuration:

```toml
[security.bootstrap_bot_admins]
telegram = [123456789]

[[policy.rules]]
id = "telegram-public"
priority = 100
disable_commands = ["kemono"]

[policy.rules.when]
platform = "telegram"
private_chat = false
```

`RUST_LOG` is consulted when `--log-filter` is absent. Tokens must never be written into the TOML
file in production and are never printed by the application.

## Shutdown

The process stops on `SIGINT` or `SIGTERM`. Shutdown steps share one deadline
(`app.shutdown_timeout_secs`, default 15s):

1. stop inbound polling / gateway events;
2. finish the in-flight message;
3. drain queued outbound sends;
4. shut down adapters;
5. close SQLite.

If the deadline expires, remaining tasks are aborted and the shortfall is logged and included in
`ShutdownReport`. A warning about abandoned tasks means the timeout should be investigated, not
silently increased.

## SQLite

Defaults:

- WAL journal mode and `synchronous=NORMAL`;
- foreign keys enabled;
- `busy_timeout_ms = 5000`;
- pool size 5.

The database path's parent directory is created when `storage.create_parent_dirs = true`. Backup
must include the WAL sidecar files while the process is running:

```bash
# Preferred: stop the bot, then copy the database.
cp data/lukosbot.sqlite3 /backup/lukosbot-$(date +%F).sqlite3

# If copying while running, include -wal and -shm.
cp data/lukosbot.sqlite3* /backup/
```

Migrations are embedded in the binary and run at startup, so a failed migration prevents the
application from serving. `lukosbot migrate` applies them explicitly without connecting to any
platform. The current schema tracks envelope `schema_version` separately from record `revision`; the
`0002` migration upgrades an existing `0001` database in place.

## H2 state migration

JVM `bot_state` cannot be opened as a SQLite file. Export it on the JVM side through JDBC to the
JSONL format in [docs/migrations/h2-state-export-format.md](migrations/h2-state-export-format.md),
then validate and import it. The repository includes a Bash wrapper around the Java 25
compact-source JDBC exporter; the wrapper requires JDK 25 or newer:

```bash
H2_JAR=/path/to/h2.jar \
H2_URL="jdbc:h2:file:/path/to/data/lukosbot2" \
H2_OUTPUT="/path/to/bot-state.jsonl" \
./tools/h2-state-export/run-h2-state-export.sh
```

Then validate and import:

```bash
# Parse, map scopes, and report conflicts without writing state records.
lukosbot import-state --input bot-state.jsonl

# Import inside one transaction. The default conflict mode is fail.
lukosbot import-state --input bot-state.jsonl --apply

# Explicitly choose conflict handling.
lukosbot import-state --input bot-state.jsonl --apply --on-conflict skip
lukosbot import-state --input bot-state.jsonl --apply --on-conflict replace
```

Before `--apply`, stop the JVM writer and back up both databases. Keep the JVM database and the
pre-import SQLite file as rollback sources. Do not run JVM and Rust instances as long-term dual
writers against the same logical state. Import reports counts only; state values are never printed.

For rollback, stop the Rust process, restore the pre-import SQLite database, and restart the JVM
version.

## Runtime dependencies

The Discord gateway uses the system TLS implementation through `native-tls` (OpenSSL on Linux).
Install the OpenSSL runtime (`libssl3` on Debian/Ubuntu) alongside the binary. The shared HTTP
client used by future command services uses Rustls and does not depend on OpenSSL.

## Logging

- `logging.format = "json"` for log collectors; `"pretty"` or `"compact"` for terminals.
- Default filter: `info,sqlx=warn`.
- Startup records the version, environment, enabled platforms, and configuration path.
- Shutdown logs a metric summary (received, handled, failed, sent, dropped).
- Secrets, message bodies, and full request URLs are not logged.

## Known limitations

- Text command engine, basic commands, authorization, policy, and the `/admin`/`/pref` framework are
  available. Media, service scheduling, and most JVM business commands are not implemented.
- No production `StateDefinition` is registered yet, so `/pref list` is intentionally empty outside
  tests. `/pref` is a framework, not a claim that JVM preference items have been migrated.
- `/weather` is a placeholder; `/whois` and other external integrations are not registered.
- Discord native slash commands are not implemented; use text prefix commands.
- Adapters support text only for outbound messages; media returns `PlatformError::Unsupported`.
- Telegram uses long polling (no webhook mode).
- Discord requires the privileged `MESSAGE_CONTENT` intent, and chat-admin checks require the
  relevant guild/channel/member data to be present in the gateway cache.
- One dispatcher serialises all messages; per-chat concurrency is future work.
- SQLite writes assume a single bot process. `tokio::Mutex` does not protect multi-process writers.
- Legacy environment variable names (`TELEGRAM_BOT_TOKEN`, ...) are not mapped; use the
  `LUKOSBOT__...` namespace.
- `/service` is not registered; the `commands.control.service` flag is reserved for the future
  service subsystem.
