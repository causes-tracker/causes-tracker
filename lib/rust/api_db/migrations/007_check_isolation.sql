-- Enforce snapshot isolation for journal writes.
-- Called by write operations on journal tables to ensure the causal
-- ordering guarantee required by the replication protocol.
-- See designdocs/Replication.md for details.
CREATE FUNCTION check_repeatable_read() RETURNS void AS $$
BEGIN
  IF current_setting('transaction_isolation') NOT IN
     ('repeatable read', 'serializable') THEN
    RAISE EXCEPTION
      'journal writes require repeatable read or higher isolation';
  END IF;
END;
$$ LANGUAGE plpgsql;
