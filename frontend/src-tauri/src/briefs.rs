//! Private preparation notes keyed by the calendar occurrence or the CRM task.
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Brief {
    pub notes: String,
    pub version: i64,
}

#[tauri::command]
pub async fn meeting_brief(state: State<'_, AppState>, key: String) -> Result<Brief, String> {
    Ok(
        sqlx::query_as("SELECT notes,version FROM meeting_briefs WHERE item_key=?")
            .bind(key)
            .fetch_optional(state.db_manager.pool())
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or(Brief {
                notes: String::new(),
                version: 0,
            }),
    )
}

#[tauri::command]
pub async fn save_meeting_brief(
    state: State<'_, AppState>,
    key: String,
    notes: String,
    version: i64,
) -> Result<Brief, String> {
    if key.is_empty() || notes.len() > 128 * 1024 {
        return Err("Check the meeting reference and note length.".into());
    }
    let pool = state.db_manager.pool();
    let row = if version == 0 {
        sqlx::query_as("INSERT INTO meeting_briefs(item_key,notes,version) VALUES(?,?,1) ON CONFLICT DO NOTHING RETURNING notes,version").bind(key).bind(notes).fetch_optional(pool).await
    } else {
        sqlx::query_as("UPDATE meeting_briefs SET notes=?,version=version+1 WHERE item_key=? AND version=? RETURNING notes,version").bind(notes).bind(key).bind(version).fetch_optional(pool).await
    };
    row.map_err(|e| e.to_string())?.ok_or(
        "Another editor updated these notes. Reopen the brief to see the saved version.".into(),
    )
}

#[tauri::command]
pub fn dial_phone(number: String) -> Result<(), String> {
    let digits = number.strip_prefix('+').unwrap_or(&number);
    if !(7..=15).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Check the phone number.".into());
    }
    #[cfg(windows)]
    {
        std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", &format!("tel:{number}")])
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(windows))]
    Err("The system dialer is available on Windows.".into())
}

#[derive(Serialize, sqlx::FromRow)]
pub struct CallAttempt {
    meeting_id: String,
    title: String,
    created_at: Option<String>,
    outcomes: sqlx::types::Json<Vec<serde_json::Value>>,
}

#[tauri::command]
pub async fn call_attempts(
    state: State<'_, AppState>,
    key: String,
) -> Result<Vec<CallAttempt>, String> {
    sqlx::query_as("SELECT r.meeting_id,m.title,m.created_at,r.outcomes FROM meeting_reviews r JOIN meetings m ON m.id=r.meeting_id WHERE r.brief_key=? ORDER BY m.created_at DESC LIMIT 20")
        .bind(key).fetch_all(state.db_manager.pool()).await.map_err(|e|e.to_string())
}

#[tauri::command]
pub fn open_prep_source(link: String) -> Result<(), String> {
    let url = url::Url::parse(&link).map_err(|_| "Invalid source link")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Choose an HTTPS source link.".into());
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(url.as_str())
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
