## db\_connect

Cloud-agnostic dispatch to the right `api_db::DbPool` constructor.

Key decisions:

- **Routing lives above the provider crates, not inside them.**
  Each `db_<cloud>` crate (currently just `db_aws`) exposes only its own connect functions.
  This crate decides which to call based on the four config knobs (`db_host`, `db_user`, `db_port`, `database_url`).
  Adding `db_azure` is a new variant on the internal `ConnectMode` enum + a new match arm — no changes to existing provider crates.
- **Binaries depend only on `db_connect`, not on individual provider crates.**
  Keeps `services/causes_api`, `services/causes_cli`, etc. from having to know about cloud-specific deps.
