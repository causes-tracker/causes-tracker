-- Stores this instance's stable identity and opaque key-value config.
-- Written once during bootstrap; never truncated.
CREATE TABLE instance_config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Local user record.
-- Aligns with the User proto (identity.proto): id maps to UserId.value,
-- auth_provider records the issuer (e.g. "accounts.google.com").
CREATE TABLE users (
    id            TEXT        PRIMARY KEY,  -- URL-safe UUID v4
    display_name  TEXT        NOT NULL,
    email         TEXT        NOT NULL,
    auth_provider TEXT        NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Maps (issuer, subject) pairs to local users.
-- Supports multiple IdPs per account (ADR-010).
-- Queryable via AdminService.GetUserIdentities — direct DB access is never needed.
CREATE TABLE external_identities (
    issuer   TEXT NOT NULL,
    subject  TEXT NOT NULL,
    user_id  TEXT NOT NULL REFERENCES users(id),
    PRIMARY KEY (issuer, subject)
);

-- Role assignments for users.
-- project_id '' means an instance-level role; otherwise a project UUID.
-- Roles from ADR-010: instance-admin, developer, project-maintainer,
-- security-team, authenticated, anonymous.
CREATE TABLE role_assignments (
    user_id    TEXT NOT NULL REFERENCES users(id),
    project_id TEXT NOT NULL DEFAULT '',  -- '' for instance-level roles
    role       TEXT NOT NULL,
    PRIMARY KEY (user_id, project_id, role)
);
