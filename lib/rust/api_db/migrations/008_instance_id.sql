-- Generate and store the instance's stable identity (UUID v4).
-- This runs once during migration; the value never changes.
-- See ADR-010 and designdocs/Replication.md.
INSERT INTO instance_config (key, value)
VALUES ('instance_id', gen_random_uuid()::text)
ON CONFLICT (key) DO NOTHING;
