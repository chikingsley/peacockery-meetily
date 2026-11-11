use crate::database::manager::DatabaseManager;

/// Managed at startup, before any command can run.
pub struct AppState {
    pub db_manager: DatabaseManager,
}
