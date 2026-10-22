-- Initial generic state table.
--
-- Timestamps are Unix milliseconds (UTC). The primary key covers the full logical key,
-- including platform, so a Telegram chat and a Discord channel with the same numeric ID
-- never collide.
CREATE TABLE IF NOT EXISTS state (
    scope_type     TEXT    NOT NULL CHECK (scope_type IN ('global', 'chat', 'user')),
    scope_platform TEXT    NOT NULL DEFAULT '',
    scope_id       TEXT    NOT NULL DEFAULT '',
    namespace      TEXT    NOT NULL,
    "key"          TEXT    NOT NULL,
    value_json     TEXT    NOT NULL,
    version        INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    expires_at     INTEGER,
    PRIMARY KEY (scope_type, scope_platform, scope_id, namespace, "key")
) STRICT;

CREATE INDEX IF NOT EXISTS idx_state_expires_at
    ON state (expires_at)
    WHERE expires_at IS NOT NULL;
