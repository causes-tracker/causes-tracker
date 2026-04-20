-- Trigger-compatible wrapper around check_repeatable_read().
-- Every journal table attaches a BEFORE INSERT OR UPDATE trigger that calls
-- this wrapper, so writes outside REPEATABLE READ are rejected before any
-- row is modified.
CREATE FUNCTION check_repeatable_read_trigger() RETURNS trigger AS $$
BEGIN
    PERFORM check_repeatable_read();
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
