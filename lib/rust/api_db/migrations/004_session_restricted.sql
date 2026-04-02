-- Add a restricted flag to sessions.
-- Restricted sessions suppress elevated roles (e.g. instance-admin).
-- Default true: existing and new sessions are restricted unless explicitly
-- created as unrestricted (admin).
ALTER TABLE sessions ADD COLUMN restricted BOOLEAN NOT NULL DEFAULT true;
