# ADR-0003: State storage

- Status: accepted
- Date: 2026-10-23

## Context

Preferences, administrators, and service state all need persistence. Business code must not depend
on SQLx directly, and the foundation must not anticipate the complete JVM state model. The existing
data lives in an H2 database and cannot be used as a SQLite file.

## Decision

Define a small `StateStore` port in `lukosbot-core`:

```rust
get(key) -> Option<VersionedValue>
put(key, value, expires_at)
delete(key)
delete_expired(now)
close()
```

A key is a `StateScope` (`Global`, `Chat`, `User`) plus a namespace and a key. Chat scope contains
`platform + chat_id`, not `ChatKind`: classification changes must not orphan state. Values are
opaque `serde_json::Value` envelopes with a version, `updated_at`, and optional expiry.

### SQLite schema

One `STRICT` table:

```sql
state
(scope_type, scope_platform, scope_id, namespace, "key",
      value_json, version, created_at, updated_at, expires_at)
PRIMARY KEY (scope_type, scope_platform, scope_id, namespace, "key")
```

- Platform is a separate column instead of being encoded into `scope_id`; no escaping rules and
  simpler indexes.
- Timestamps are Unix milliseconds (`INTEGER`) in UTC; formatting cannot drift between writers and a
  range index works naturally.
- `get` hides expired rows; `delete_expired` removes them physically. No periodic task is scheduled
  yet.
- Upserts preserve `created_at` and update the value, version, `updated_at`, and expiry.
- WAL, `synchronous=NORMAL`, foreign keys, and `busy_timeout` are configured on connection.
- Migrations are embedded with `sqlx::migrate!` in `lukosbot-infra/migrations` and run at startup
  and through `lukosbot migrate`.

Compile-time checked SQL is not used, so CI needs neither a database nor `.sqlx` metadata; the SQL
is exercised against real temporary SQLite files.

## Consequences

- Handlers can depend on `StateStore` and be tested with `MemoryStateStore`.
- The storage schema is deliberately generic; domain-specific state definitions, precedence, and
  permissions belong to later plans and can be layered on this port.
- Importing the H2 data and switching production traffic is a separate migration project with its
  own validation and rollback plan.
