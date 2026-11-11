use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use tauri::{AppHandle, Manager, Runtime};

const DATABASE_FILE: &str = "peacockery-meetily.sqlite";

#[derive(Clone)]
pub struct DatabaseManager {
    pool: SqlitePool,
}

impl DatabaseManager {
    /// Open (creating if missing) the database at `path` and run the migrations.
    pub async fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .map_err(|e| format!("Failed to open database {}: {}", path.display(), e))?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|e| format!("Failed to migrate database: {}", e))?;
        log::info!("Database ready at {}", path.display());
        Ok(Self { pool })
    }

    /// Open the app database in the app data directory.
    pub async fn open_for_app<R: Runtime>(app: &AppHandle<R>) -> Result<Self, String> {
        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
        Self::open(&app_data_dir.join(DATABASE_FILE)).await
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Checkpoint the WAL into the main file and close the pool (app exit).
    pub async fn cleanup(&self) {
        if let Err(e) = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&self.pool).await {
            log::warn!("WAL checkpoint failed: {}", e);
        }
        self.pool.close().await;
        log::info!("Database closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_creates_and_migrates_a_file_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(DATABASE_FILE);
        let db = DatabaseManager::open(&path).await.unwrap();
        assert!(path.is_file());
        let tables: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE '\\_%' ESCAPE '\\' ORDER BY name")
                .fetch_all(db.pool())
                .await
                .unwrap();
        assert_eq!(tables, vec!["meetings", "summary_processes", "transcripts"]);
        db.cleanup().await;

        // Re-opening an existing database is a no-op migration.
        let db = DatabaseManager::open(&path).await.unwrap();
        db.cleanup().await;
    }
}
