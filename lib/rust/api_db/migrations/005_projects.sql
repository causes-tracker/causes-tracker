CREATE TABLE projects (
    id                   TEXT        PRIMARY KEY,
    name                 TEXT        NOT NULL UNIQUE,
    description          TEXT        NOT NULL DEFAULT '',
    visibility           TEXT        NOT NULL DEFAULT 'private',
    embargoed_by_default BOOLEAN     NOT NULL DEFAULT false,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);
