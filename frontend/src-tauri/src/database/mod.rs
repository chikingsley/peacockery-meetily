pub mod manager;
pub mod models;
pub mod repositories;

/// Current time as stored in the database (RFC 3339, UTC).
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// In-memory database with the real migrations, for tests.
#[cfg(test)]
pub(crate) async fn test_pool() -> sqlx::SqlitePool {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .expect("valid sqlite url")
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("in-memory database");
    sqlx::migrate!("./migrations").run(&pool).await.expect("migrations");
    pool
}
