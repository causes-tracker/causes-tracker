## db\_pool

Opaque database pool handle for the Causes API stack.

Key decisions:

- **AI guardrail: sqlx access requires both this crate AND the sealed `QueryAccess` trait in scope.**
  Crates that legitimately write SQL (api_db today; future analytics_db etc.) depend on this crate and `use db_pool::QueryAccess` to reach `pool()` / the underlying `sqlx::PgPool`.
  Business-logic crates (services/causes_api etc.) depend on `db_connect` for the type only and never see the trait, so they cannot construct queries even if an AI tries to write them.
- **The pool type is opaque to consumers.**
  Consumers see `Clone`, `start_background_refresh`, and the type's existence — nothing about sqlx.
- **Refreshers only get the `SetConnectOptions` capability.**
  `start_background_refresh` hands the refresher `&dyn SetConnectOptions`, never the pool, so credential rotation (production IAM tokens) can install new connect options but cannot query or connect.
- **No transaction policy here.**
  Isolation level is a schema concern (e.g. api_db's journal trigger requires REPEATABLE READ); each query crate owns its own `begin_txn` helper.
