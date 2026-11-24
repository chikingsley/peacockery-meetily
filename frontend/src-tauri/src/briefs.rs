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

#[cfg(any(windows, test))]
fn google_voice_url(number: &str, account: u64) -> Result<String, String> {
    let digits = number.strip_prefix('+').unwrap_or(number);
    if !(7..=15).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Check the phone number.".into());
    }
    let prefix = if number.starts_with('+') || (digits.len() == 11 && digits.starts_with('1')) {
        "%2B"
    } else if digits.len() == 10 {
        "%2B1"
    } else {
        return Err("Use a phone number with its country code.".into());
    };
    Ok(format!(
        "https://voice.google.com/u/{account}/calls?a=nc,{prefix}{digits}"
    ))
}

#[tauri::command]
pub fn dial_phone(app: tauri::AppHandle, number: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        use tauri_plugin_store::StoreExt;
        let account = app
            .store("recording_preferences.json")
            .map_err(|e| e.to_string())?
            .get("google_voice_account")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let url = google_voice_url(&number, account)?;
        let chrome = [
            "LOCALAPPDATA",
            "ProgramW6432",
            "ProgramFiles",
            "ProgramFiles(x86)",
        ]
        .iter()
        .filter_map(std::env::var_os)
        .map(|root| std::path::PathBuf::from(root).join("Google/Chrome/Application/chrome.exe"))
        .find(|path| path.is_file())
        .ok_or("Install Google Chrome to open Google Voice.")?;
        std::process::Command::new(chrome)
            .arg(url)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (app, number);
        Err("Google Voice call tasks are available on Windows.".into())
    }
}

#[cfg(test)]
mod dial_tests {
    use super::google_voice_url;
    #[test]
    fn routes_to_the_selected_voice_account_with_a_country_code() {
        for n in ["2025550100", "12025550100", "+12025550100"] {
            assert_eq!(
                google_voice_url(n, 1).unwrap(),
                "https://voice.google.com/u/1/calls?a=nc,%2B12025550100"
            );
        }
        assert!(google_voice_url("+442079460000", 0)
            .unwrap()
            .ends_with("%2B442079460000"));
        for invalid in ["", "5550100", "+1&extra", "1+2025550100"] {
            assert!(google_voice_url(invalid, 0).is_err());
        }
    }
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
