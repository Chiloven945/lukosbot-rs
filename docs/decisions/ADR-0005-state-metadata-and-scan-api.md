# ADR-0005: State metadata and namespace scans

- Status: accepted
- Date: 2026-10-23

## Context

The initial state table stored an envelope `version` that was always written as `1`, exposed no
`created_at`, and offered no way to load a namespace. Service state, administrator state, and future
preference tooling need namespace queries, and the H2 migration must preserve the old record
`version` separately from the JSON format version.

## Decision

Extend `StateStore` with:

```rust
list_namespace(scope, namespace) -> Vec<StateEntry>
scan_namespace(scope_kind, namespace) -> Vec<StateEntry>
```

Both methods hide expired rows and return deterministic ordering. `StateEntry` carries the full
`StateKey` plus `VersionedValue`.

`VersionedValue` separates:

- `schema_version`: JSON envelope format, currently `1`;
- `revision`: number of updates, starting at `0` for new records and incremented on update;
- `created_at`: preserved on update;
- `updated_at`, `expires_at`, and the JSON value.

Migration `0002` renames the old `version` column to `schema_version`, adds `revision INTEGER NOT
NULL DEFAULT 0`, and adds a `(scope_type, namespace, scope_platform, scope_id, key)` index. Existing
rows retain their data and `created_at`; their old version becomes the envelope version.

The H2 importer writes records through a dedicated infrastructure transaction rather than
`StateStore::put`: `put` intentionally manages revision and timestamps for normal runtime writes,
while migration must preserve imported revision and creation time.

## Consequences

- Normal writes and imports have distinct semantics and cannot accidentally share a lossy path.
- `MemoryStateStore` must implement the same revision, creation-time, expiry, list, and scan
  behavior as SQLite.
- An old `0001` database upgrades in place; tests cover the upgrade with existing data.
- State consumers can load `service` and other namespaces without adding SQL to business code.
- Multi-process writers remain out of scope; list/scan are not a distributed consistency mechanism.
