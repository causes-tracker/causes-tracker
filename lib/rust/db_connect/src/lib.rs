//! Cloud-agnostic dispatch to the right `db_pool::DbPool` constructor.
//!
//! Binaries pass the same four `Option`s they parse from env/CLI; this
//! crate decides whether to use a cloud-IAM auth flow or a static
//! `DATABASE_URL`. Adding a new cloud (e.g. Azure managed identity) is a
//! new variant on [`ConnectMode`] + a new arm in [`connect`].

use anyhow::Context;

/// Re-export so binaries can `use db_connect::DbPool` without depending on
/// `db_pool` directly. The sealed `QueryAccess` trait is not re-exported,
/// which means business-logic crates cannot reach the underlying sqlx pool.
pub use db_pool::DbPool;

/// The auth flow chosen from configuration. Pulled out of [`connect`] so
/// the routing decision is unit-testable without a cloud provider chain.
#[derive(Debug, PartialEq, Eq)]
enum ConnectMode {
    AwsIam {
        host: String,
        user: String,
        port: u16,
    },
    Static {
        url: String,
    },
}

fn connect_mode(
    db_host: Option<&str>,
    db_user: Option<&str>,
    db_port: u16,
    database_url: Option<&str>,
) -> anyhow::Result<ConnectMode> {
    match (db_host, db_user) {
        (Some(host), Some(user)) => Ok(ConnectMode::AwsIam {
            host: host.to_owned(),
            user: user.to_owned(),
            port: db_port,
        }),
        _ => {
            let url = database_url
                .context("DATABASE_URL is required when DB_HOST/DB_USER are not set")?;
            Ok(ConnectMode::Static {
                url: url.to_owned(),
            })
        }
    }
}

/// Build a [`db_pool::DbPool`] from environment-style configuration,
/// attaching the consumer's default pool state `E`.
#[tracing::instrument(skip(database_url), fields(db.system = "postgresql"))]
pub async fn connect<E: Default>(
    db_host: Option<&str>,
    db_user: Option<&str>,
    db_port: u16,
    database_url: Option<&str>,
) -> anyhow::Result<db_pool::DbPool<E>> {
    match connect_mode(db_host, db_user, db_port, database_url)? {
        ConnectMode::AwsIam { host, user, port } => {
            tracing::info!("using AWS IAM database authentication");
            db_aws::connect_iam(&host, port, &user, E::default()).await
        }
        ConnectMode::Static { url } => db_pool::DbPool::connect(&url, E::default()).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_aws_iam_when_host_and_user_are_set() {
        let mode = connect_mode(Some("db.example"), Some("svc"), 5432, None).unwrap();
        assert_eq!(
            mode,
            ConnectMode::AwsIam {
                host: "db.example".into(),
                user: "svc".into(),
                port: 5432,
            }
        );
    }

    #[test]
    fn picks_aws_iam_even_when_database_url_also_set() {
        // IAM takes precedence — production sets all three.
        let mode =
            connect_mode(Some("db.example"), Some("svc"), 5432, Some("postgres://x")).unwrap();
        assert!(matches!(mode, ConnectMode::AwsIam { .. }));
    }

    #[test]
    fn falls_back_to_static_url_when_host_missing() {
        let mode = connect_mode(None, Some("svc"), 5432, Some("postgres://localhost/db")).unwrap();
        assert_eq!(
            mode,
            ConnectMode::Static {
                url: "postgres://localhost/db".into()
            }
        );
    }

    #[test]
    fn falls_back_to_static_url_when_user_missing() {
        let mode = connect_mode(
            Some("db.example"),
            None,
            5432,
            Some("postgres://localhost/db"),
        )
        .unwrap();
        assert_eq!(
            mode,
            ConnectMode::Static {
                url: "postgres://localhost/db".into()
            }
        );
    }

    #[test]
    fn errors_when_no_credentials_and_no_url() {
        let err = connect_mode(None, None, 5432, None).unwrap_err();
        assert!(
            err.to_string().contains("DATABASE_URL is required"),
            "unexpected error: {err}"
        );
    }
}
