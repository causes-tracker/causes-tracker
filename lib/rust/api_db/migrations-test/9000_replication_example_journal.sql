-- Journal table for the ReplicationExample scaffolding resource.
-- Demonstrates `journal_create_table()` from migration 010: every
-- per-resource journal table is one call, varying only name and payload.
-- This table will be removed once real resource types (Plan etc.) exist.
SELECT journal_create_table(
    'replication_example_journal',
    'payload TEXT NOT NULL'
);
