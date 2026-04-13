-- Convert the empty-string sentinel for instance-level roles to NULL,
-- make project_id nullable, and add a foreign key to the projects table.
--
-- The primary key includes project_id, so we drop and recreate it.
-- NULL is distinct in unique constraints, so (user_id, NULL, role) won't
-- conflict with (user_id, NULL, role) — we add a partial unique index
-- for instance-level rows to prevent duplicates.

ALTER TABLE role_assignments DROP CONSTRAINT role_assignments_pkey;

ALTER TABLE role_assignments
    ALTER COLUMN project_id DROP DEFAULT,
    ALTER COLUMN project_id DROP NOT NULL;

UPDATE role_assignments SET project_id = NULL WHERE project_id = '';

ALTER TABLE role_assignments
    ADD CONSTRAINT role_assignments_project_id_fkey
        FOREIGN KEY (project_id) REFERENCES projects(id);

-- Project-scoped: one row per (user, project, role).
CREATE UNIQUE INDEX role_assignments_project_uq
    ON role_assignments (user_id, project_id, role)
    WHERE project_id IS NOT NULL;

-- Instance-level: one row per (user, role).
CREATE UNIQUE INDEX role_assignments_instance_uq
    ON role_assignments (user_id, role)
    WHERE project_id IS NULL;
