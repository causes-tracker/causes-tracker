-- Opaque session tokens issued by the instance after a successful login.
-- Token is a hex-encoded random string (32 bytes / 64 hex chars).
-- Server-side storage enables immediate revocation and claims resolution
-- without embedding anything in the token itself (ADR-010).
CREATE TABLE sessions (
    token       TEXT        PRIMARY KEY,
    user_id     TEXT        NOT NULL REFERENCES users(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL
);

CREATE INDEX sessions_user_id_idx ON sessions (user_id);