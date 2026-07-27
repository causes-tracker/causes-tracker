# tools/sqlx-cli

Thin `main.rs` wrapping the `sqlx-cli` crate, compiled hermetically as `:sqlx_bin`.
Run via the wrapper at `:sqlx` (`bazel run //tools/sqlx-cli:sqlx -- migrate run`), not directly — the wrapper `cd`s to the repo root before exec so sqlx-cli's own `.env` lookup finds `DATABASE_URL`.
