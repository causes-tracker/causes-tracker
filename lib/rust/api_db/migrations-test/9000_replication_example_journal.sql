-- Journal table for the ReplicationExample scaffolding resource.
-- This table will be removed when real resource types (Plans etc.) exist.
-- The schema serves as the reference shape for per-resource journal tables.

CREATE TABLE replication_example_journal (
    -- JournalEntryHeader: federation identity
    origin_instance_id TEXT   NOT NULL,
    origin_id          TEXT   NOT NULL,
    version            BIGINT NOT NULL,

    -- JournalEntryHeader: previous version (nullable triple — all-or-none)
    previous_origin_instance_id TEXT,
    previous_origin_id          TEXT,
    previous_version            BIGINT,

    -- JournalEntryHeader: other fields
    kind                TEXT        NOT NULL,  -- 'entry' or 'tombstone'
    at                  TIMESTAMPTZ NOT NULL,
    author_instance_id  TEXT        NOT NULL,
    author_local_id     TEXT        NOT NULL,
    embargoed           BOOLEAN     NOT NULL,

    -- ResourceEntryMeta fields
    slug        TEXT        NOT NULL,
    project_id  TEXT        NOT NULL REFERENCES projects(id),
    created_at  TIMESTAMPTZ NOT NULL,

    -- Resource-specific fields
    payload     TEXT        NOT NULL,

    -- Replication-serving columns (Postgres-specific, not in proto)
    local_version BIGINT NOT NULL DEFAULT pg_current_xact_id()::text::bigint,
    watermark     BIGINT NOT NULL DEFAULT pg_snapshot_xmin(pg_current_snapshot())::text::bigint,

    PRIMARY KEY (origin_instance_id, origin_id, version),

    -- All three previous_* columns must be set together or none at all.
    CONSTRAINT previous_version_all_or_none CHECK (
        (previous_origin_instance_id IS NULL
            AND previous_origin_id IS NULL
            AND previous_version IS NULL)
        OR
        (previous_origin_instance_id IS NOT NULL
            AND previous_origin_id IS NOT NULL
            AND previous_version IS NOT NULL)
    )
);

-- Replication serving index: project + local commit order.
CREATE INDEX replication_example_journal_local_version_idx
    ON replication_example_journal (project_id, local_version);

-- Enforce snapshot isolation on writes.
CREATE TRIGGER replication_example_journal_check_isolation
    BEFORE INSERT OR UPDATE ON replication_example_journal
    FOR EACH STATEMENT
    EXECUTE FUNCTION check_repeatable_read_trigger();
