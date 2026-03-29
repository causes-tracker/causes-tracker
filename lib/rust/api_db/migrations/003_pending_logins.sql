-- Pending device-flow login attempts.  Persisted so that login state
-- survives server restarts — the CLI polls CompleteLogin with the nonce.
-- Rows are deleted on successful login or expiry.
CREATE TABLE pending_logins (
    nonce         TEXT        PRIMARY KEY,
    device_code   TEXT        NOT NULL,
    interval_secs INT         NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at    TIMESTAMPTZ NOT NULL
);
