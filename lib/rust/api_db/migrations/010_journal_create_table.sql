-- DRY the per-resource journal table DDL.
--
-- Every resource type (Plan, Sign, Symptom, Comment, ...) has its own
-- journal table.  The 14 meta columns, replication-serving columns,
-- previous_version constraint, primary key, index, and isolation trigger
-- are identical across all such tables — only the table name and the
-- payload columns differ.  This function creates a journal table with
-- the canonical shape, given a name and an SQL fragment for the payload.
--
-- Per-resource migrations look like:
--   SELECT journal_create_table(
--       'plan_journal',
--       'title TEXT NOT NULL, status TEXT NOT NULL, ...'
--   );
CREATE FUNCTION journal_create_table(table_name TEXT, payload_ddl TEXT) RETURNS void AS $$
BEGIN
    EXECUTE format(
        'CREATE TABLE %I (
            origin_instance_id          TEXT NOT NULL,
            origin_id                   TEXT NOT NULL,
            version                     BIGINT NOT NULL,
            previous_origin_instance_id TEXT,
            previous_origin_id          TEXT,
            previous_version            BIGINT,
            kind                        TEXT NOT NULL,
            at                          TIMESTAMPTZ NOT NULL,
            author_instance_id          TEXT NOT NULL,
            author_local_id             TEXT NOT NULL,
            embargoed                   BOOLEAN NOT NULL,
            slug                        TEXT NOT NULL,
            project_id                  TEXT NOT NULL REFERENCES projects(id),
            created_at                  TIMESTAMPTZ NOT NULL,
            local_version BIGINT NOT NULL DEFAULT pg_current_xact_id()::text::bigint,
            watermark     BIGINT NOT NULL DEFAULT pg_snapshot_xmin(pg_current_snapshot())::text::bigint,
            %s,
            PRIMARY KEY (origin_instance_id, origin_id, version),
            CONSTRAINT %I CHECK (
                (previous_origin_instance_id IS NULL
                    AND previous_origin_id IS NULL
                    AND previous_version IS NULL)
                OR
                (previous_origin_instance_id IS NOT NULL
                    AND previous_origin_id IS NOT NULL
                    AND previous_version IS NOT NULL)
            )
        )',
        table_name,
        payload_ddl,
        table_name || '_prev_all_or_none'
    );

    EXECUTE format(
        'CREATE INDEX %I ON %I (project_id, local_version)',
        table_name || '_local_version_idx',
        table_name
    );

    EXECUTE format(
        'CREATE TRIGGER %I
            BEFORE INSERT OR UPDATE ON %I
            FOR EACH STATEMENT
            EXECUTE FUNCTION check_repeatable_read_trigger()',
        table_name || '_check_isolation',
        table_name
    );
END;
$$ LANGUAGE plpgsql;
