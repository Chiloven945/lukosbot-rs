# H2 `bot_state` export format

Rust imports state from a UTF-8 JSONL file produced by a JVM-side JDBC export. The H2 database file
is never opened by the Rust process.

## Header

The first non-empty line must be exactly one header object:

```json
{
  "record": "header",
  "format": "lukosbot-state-export",
  "version": 1,
  "exported_at": "2026-10-23T00:00:00Z"
}
```

- `format` must be `lukosbot-state-export`.
- `version` is currently `1`.
- Timestamps are UTC RFC 3339.

## State record

Every following non-empty line is one state record:

```json
{
  "record": "state",
  "scope_type": "GLOBAL",
  "scope_id": "_",
  "namespace": "auth",
  "key": "bot_admins",
  "value": {
    "TELEGRAM": [
      123456789
    ]
  },
  "revision": 2,
  "created_at": "2026-10-20T10:00:00Z",
  "updated_at": "2026-10-22T10:00:00Z",
  "expires_at": null
}
```

- `scope_type` is the legacy value (`GLOBAL`, `USER`, `CHAT`); matching is case-insensitive.
- `scope_id` is the legacy scope ID.
- `namespace` and `key` are preserved verbatim.
- `value` is the parsed JSON value, not a JSON-encoded string. The importer stores an opaque JSON
  value and does not require knowledge of service or Cave structures.
- `revision` is the old `bot_state.version` value. It maps to the new record `revision`, not to the
  envelope `schema_version`.
- `created_at` and `updated_at` are preserved. `expires_at` may be `null`.

The importer rejects invalid JSON, missing headers, unknown scope kinds, unsupported versions, and
duplicate source keys. It reports only keys and line numbers, never full values.

## Scope mapping

| H2 `scope_type` / `scope_id`  | Rust `StateScope`              |
|-------------------------------|--------------------------------|
| `GLOBAL` / `_`                | `Global`                       |
| `USER` / `TELEGRAM:123`       | `User { Telegram, "123" }`     |
| `USER` / `DISCORD:123`        | `User { Discord, "123" }`      |
| `CHAT` / `TELEGRAM:g:-100123` | `Chat { Telegram, "-100123" }` |
| `CHAT` / `TELEGRAM:p:123`     | `Chat { Telegram, "123" }`     |
| `CHAT` / `DISCORD:g:123`      | `Chat { Discord, "123" }`      |

The legacy `g`/`p` marker is metadata and is dropped. If two legacy rows map to the same new key,
the import fails by default; it never silently chooses one.

## Export query shape

A JDBC exporter should read `bot_state` in a stable order and write one JSON object per line. The
important fields are:

```sql
SELECT scope_type,
       scope_id,
       namespace,
       k,
       v_json,
       version,
       created_at,
       updated_at,
       expires_at
FROM bot_state
ORDER BY scope_type, scope_id, namespace, k;
```

`v_json` must be embedded as raw JSON in the record's `value` field. If the JVM JSON library cannot
validate it, the export should stop rather than write a malformed line. Convert `TIMESTAMP` values
to UTC `Instant`/RFC 3339. Do not export the H2 file itself.

A minimal JDBC reference exporter is available at
[`tools/h2-state-export/H2StateExport.java`](../../tools/h2-state-export/H2StateExport.java). It is
a migration tool and is not part of the Cargo workspace. The Rust importer's fixtures in
`crates/infra/tests/import.rs` are the compatibility contract.

The exporter is a Java 25 compact source file. Run it through the Bash wrapper, which launches the
source directly with the H2 JDBC driver and requires JDK 25 or newer:

```bash
H2_JAR=/path/to/h2.jar \
H2_URL="jdbc:h2:file:/path/to/data/lukosbot2" \
H2_OUTPUT="/path/to/bot-state.jsonl" \
./tools/h2-state-export/run-h2-state-export.sh
```

`--jar`, `--url`, `--output`, `--user`, and `--password` override the corresponding environment
variables. `H2_JAR` is auto-detected from the script directory, the current directory, or the local
Maven/Gradle caches when omitted. See
[`tools/h2-state-export/README.md`](../../tools/h2-state-export/README.md) for the full options and
manual fallback.

## Import operation

```bash
# Dry-run: validates, maps scopes, and reports conflicts without writing state records.
lukosbot import-state --input bot-state.jsonl

# Apply in one transaction. Default conflict mode is fail.
lukosbot import-state --input bot-state.jsonl --apply

# Explicit conflict handling.
lukosbot import-state --input bot-state.jsonl --apply --on-conflict skip
lukosbot import-state --input bot-state.jsonl --apply --on-conflict replace
```

`skip` keeps the existing target record. `replace` overwrites the target record, including imported
metadata. `fail` rolls back the entire import. The target schema is migrated before import; dry-run
therefore may update schema metadata but never writes state records.

Before applying:

1. stop the JVM writer;
2. back up the H2 database and target SQLite file, including WAL sidecars;
3. run a dry-run and resolve reported conflicts;
4. apply the import;
5. sample-read key namespaces such as `auth`, `service`, and Cave metadata;
6. start only one writer (the Rust process).

Rollback is restoring the pre-import SQLite backup and restarting the JVM version.
