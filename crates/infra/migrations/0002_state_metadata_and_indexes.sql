-- Split the envelope schema version from the record revision, and add the namespace scan index.
--
-- The existing `version` column was always written as the envelope version. Existing rows keep
-- that value as `schema_version`; `revision` starts at 0 for rows that predate revision tracking.
ALTER TABLE state RENAME COLUMN version TO schema_version;

ALTER TABLE state
    ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_state_scope_namespace
    ON state (scope_type, namespace, scope_platform, scope_id, "key");
