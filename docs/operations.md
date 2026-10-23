# Operations

## Process interface

```text
lukosbot [--config PATH] [--log-filter FILTER] <run|check-config|migrate>
```

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
platform.

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

- Text command engine and basic commands are available; permissions/policy, media pipeline, and H2
  data import are not yet implemented.
- `/weather` is a placeholder; `/whois` and other external integrations are not registered.
- Discord native slash commands are not implemented; use text prefix commands.
- Adapters support text only for outbound messages; media returns `PlatformError::Unsupported`.
- Telegram uses long polling (no webhook mode).
- Discord requires the privileged `MESSAGE_CONTENT` intent.
- One dispatcher serialises all messages; per-chat concurrency is future work.
- Legacy environment variable names (`TELEGRAM_BOT_TOKEN`, ...) are not mapped; use the
  `LUKOSBOT__...` namespace.
- `delete_expired` exists and is tested, but no periodic cleanup task is scheduled yet.
