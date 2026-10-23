import java.sql.DriverManager;
import java.sql.SQLException;
import java.sql.Statement;
import java.sql.Timestamp;

/**
 * Minimal JDBC reference exporter for the JVM bot_state table.
 * <p>
 * This is a Java 25 compact source file: it has no class declaration and is launched directly
 * with the source-file launcher. This tool is deliberately outside the Cargo workspace. Put the
 * H2 JDBC driver on the classpath and run it while the JVM bot is stopped (or at least not
 * writing):
 * <p>
 * <pre>{@code
 *   java -cp h2.jar H2StateExport.java \
 *     "jdbc:h2:file:./data/lukosbot2" "bot-state.jsonl" sa ""
 * }</pre>
 */
void main(String[] args) throws Exception {
    if (args.length < 2 || args.length > 4) {
        System.err.println(
                "usage: H2StateExport.java <jdbc-url> <output.jsonl> [user] [password]"
        );
        System.exit(2);
    }

    var url = args[0];
    var output = Path.of(args[1]);
    var user = args.length >= 3
            ? args[2]
            : "sa";
    var password = args.length >= 4
            ? args[3]
            : "";

    try (
            var connection = DriverManager.getConnection(url, user, password);
            var statement = connection.createStatement();
            var writer = Files.newBufferedWriter(output, StandardCharsets.UTF_8)
    ) {
        writer.write(header());
        writer.newLine();
        export(statement, writer);
    }
}

private static String header() {
    return /* language=JSON */ """
            {
              "record":"header",
              "format":"lukosbot-state-export",
              "version":1,
              "exported_at":%s
            }
            """.formatted(quote(Instant.now().toString()));
}

private static void export(
        Statement statement,
        BufferedWriter writer
) throws SQLException, IOException {
    try (
            var rows = statement.executeQuery("""
                    SELECT
                        scope_type,
                        scope_id,
                        namespace,
                        k,
                        v_json,
                        version,
                        created_at,
                        updated_at,
                        expires_at
                    FROM
                        bot_state
                    ORDER BY
                        scope_type,
                        scope_id,
                        namespace,
                        k
                    """
            )
    ) {
        while (rows.next()) {
            var valueJson = rows.getString("v_json");
            if (valueJson == null) {
                throw new SQLException("bot_state row has null v_json");
            }

            writer.write("{\"record\":\"state\"");
            writer.write(",\"scope_type\":" + quote(rows.getString("scope_type")));
            writer.write(",\"scope_id\":" + quote(rows.getString("scope_id")));
            writer.write(",\"namespace\":" + quote(rows.getString("namespace")));
            writer.write(",\"key\":" + quote(rows.getString("k")));
            // v_json is already a JSON value; do not quote or double-encode it.
            writer.write(",\"value\":" + valueJson);
            writer.write(",\"revision\":" + rows.getLong("version"));
            writer.write(
                    ",\"created_at\":" + quote(timestamp(rows.getTimestamp("created_at")))
            );
            writer.write(
                    ",\"updated_at\":" + quote(timestamp(rows.getTimestamp("updated_at")))
            );

            var expiresAt = rows.getTimestamp("expires_at");
            writer.write(",\"expires_at\":");
            writer.write(expiresAt == null
                    ? "null"
                    : quote(timestamp(expiresAt))
            );
            writer.write("}");
            writer.newLine();
        }
    }
}

private static String quote(String value) {
    if (value == null) {
        return "null";
    }

    var result = new StringBuilder(value.length() + 2);
    result.append('"');
    IntStream.range(0, value.length())
            .forEach(index -> {
                var character = value.charAt(index);
                switch (character) {
                    case '"' -> result.append("\\\"");
                    case '\\' -> result.append("\\\\");
                    case '\b' -> result.append("\\b");
                    case '\f' -> result.append("\\f");
                    case '\n' -> result.append("\\n");
                    case '\r' -> result.append("\\r");
                    case '\t' -> result.append("\\t");
                    default -> {
                        if (character < 0x20) {
                            result.append(String.format("\\u%04x", (int) character));
                        } else {
                            result.append(character);
                        }
                    }
                }
            });
    result.append('"');
    return result.toString();
}

private static String timestamp(Timestamp timestamp) {
    return timestamp == null
            ? null
            : timestamp.toLocalDateTime()
                    .toInstant(ZoneOffset.UTC)
                    .toString();
}
