# H2 state exporter

`H2StateExport.java` is a small JDBC reference tool for exporting the JVM `bot_state` table to the
JSONL format consumed by `lukosbot import-state`. It is a Java 25 compact source file, so it is
launched directly without a separate compilation step. It is intentionally outside the Cargo
workspace and is not part of CI.

## Requirements

- JDK 25 or newer.
- An H2 JDBC jar.

## Quick start

`run-h2-state-export.sh` runs the source file directly with the H2 JDBC driver. It does not modify
the Java source or leave build artifacts in the repository.

```bash
H2_JAR=/path/to/h2.jar \
H2_URL="jdbc:h2:file:/path/to/data/lukosbot2" \
H2_OUTPUT="/path/to/bot-state.jsonl" \
./tools/h2-state-export/run-h2-state-export.sh
```

If `H2_JAR` is omitted, the script looks for `h2.jar` next to itself, in the current directory, or
in the local Maven/Gradle caches.

Flags override environment variables:

```bash
./tools/h2-state-export/run-h2-state-export.sh \
  --jar /path/to/h2.jar \
  --url "jdbc:h2:file:/path/to/data/lukosbot2" \
  --output /path/to/bot-state.jsonl \
  --user sa
```

| Option       | Environment   | Default                         |
|--------------|---------------|---------------------------------|
| `--jar`      | `H2_JAR`      | auto-detect                     |
| `--url`      | `H2_URL`      | `jdbc:h2:file:./data/lukosbot2` |
| `--output`   | `H2_OUTPUT`   | `./bot-state.jsonl`             |
| `--user`     | `H2_USER`     | `sa`                            |
| `--password` | `H2_PASSWORD` | empty                           |

`JAVA_BIN` can override the Java executable. Run with `--help` for the full summary.

The password is passed to the existing Java exporter as a command-line argument, so avoid using
`--password` on a shared machine. Prefer `H2_PASSWORD` and stop the JVM bot before the final export.

## Manual usage

The script is a convenience wrapper. The equivalent manual command is:

```bash
java -cp /path/to/h2.jar H2StateExport.java \
  "jdbc:h2:file:./data/lukosbot2" \
  "bot-state.jsonl" \
  sa
```

The default user is `sa` and the default password is empty. Pass a fourth argument when a password
is configured.

The exporter writes `v_json` verbatim into the record's `value` field. Do not wrap it in quotes or
JSON-encode it again. The Rust importer validates every line and rejects invalid JSON.

See [docs/migrations/h2-state-export-format.md](../../docs/migrations/h2-state-export-format.md) for
the full record contract and import procedure.
