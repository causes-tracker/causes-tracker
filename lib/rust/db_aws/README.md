## db\_aws

AWS-aware connection setup for `api_db::DbPool`.

Key decisions:

- **Split from `api_db` to keep the AWS SDK off the SQL crate's compile graph.**
  `sqlx prepare` runs `cargo check` on `api_db` to validate query macros against the database; pulling in `aws-sdk-rds` + `aws-config` + their transitive crates roughly doubled cold compile time.
  Anything that talks to AWS lives here, behind the same `api_db::DbPool` opaque handle.
- **Token rotation via a refresher closure on `DbPool`.**
  `connect_iam` returns an `api_db::DbPool` whose `start_background_refresh` periodically regenerates the IAM auth token and atomically swaps in a new pool.
  `api_db` defines the `PoolRefresher` type alias and the slot to hold one; this crate is the only producer.
