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
        let mut launch = std::process::Command::new(chrome);
        let app_id = "fnlilphdnnmgngfbdofeajhloeckmcnf";
        let installed = std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .map(|dir| {
                dir.join("Google/Chrome/User Data/Default/Web Applications")
                    .join(format!("_crx_{app_id}"))
                    .is_dir()
            })
            .unwrap_or(false);
        if installed {
            launch
                .arg("--profile-directory=Default")
                .arg(format!("--app-id={app_id}"))
                .arg(format!("--app-launch-url-for-shortcuts-menu-item={url}"));
        } else {
            launch.arg(url);
        }
        launch.spawn().map_err(|e| e.to_string())?;
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

#[derive(Deserialize)]
pub struct CallContact {
    pub name: String,
    pub number: String,
}

#[tauri::command]
pub async fn set_call_outcome(
    state: State<'_, AppState>,
    meeting_id: String,
    outcome: String,
) -> Result<(), String> {
    if !matches!(
        outcome.as_str(),
        "automatic" | "conversation" | "voicemail_left" | "no_answer" | "undetermined"
    ) {
        return Err("Choose a listed outcome.".into());
    }
    let result = sqlx::query(
        "UPDATE call_destinations SET manual_outcome=NULLIF(?, 'automatic') WHERE meeting_id=?",
    )
    .bind(outcome)
    .bind(meeting_id)
    .execute(state.db_manager.pool())
    .await
    .map_err(|e| e.to_string())?;
    if result.rows_affected() != 1 {
        return Err("Call attempt unavailable".into());
    }
    Ok(())
}

#[derive(Serialize, sqlx::FromRow)]
pub struct CallAttempt {
    meeting_id: String,
    title: String,
    number: Option<String>,
    name: Option<String>,
    manual_outcome: Option<String>,
    reviewed: bool,
    editable: bool,
    created_at: Option<String>,
    outcomes: sqlx::types::Json<Vec<serde_json::Value>>,
}

#[tauri::command]
pub async fn call_attempts(
    state: State<'_, AppState>,
    key: String,
) -> Result<Vec<CallAttempt>, String> {
    attempts_for_key(state.db_manager.pool(), &key)
        .await
        .map_err(|e| e.to_string())
}

async fn attempts_for_key(
    pool: &sqlx::SqlitePool,
    key: &str,
) -> Result<Vec<CallAttempt>, sqlx::Error> {
    sqlx::query_as("SELECT m.id AS meeting_id,m.title,m.created_at,d.name,d.number,d.manual_outcome,d.meeting_id IS NOT NULL AS editable,
        COALESCE(r.revision LIKE 'end:%' OR r.attempts>=2,0) OR (m.status!='recording' AND COALESCE(m.final_status,'') NOT IN ('queued','uploading','transcribing') AND NOT EXISTS(SELECT 1 FROM transcripts t WHERE t.meeting_id=m.id)) AS reviewed,
        COALESCE(r.outcomes,'[]') AS outcomes FROM meetings m LEFT JOIN meeting_reviews r ON r.meeting_id=m.id
        LEFT JOIN call_destinations d ON d.meeting_id=m.id WHERE d.task_key=?1 OR (d.meeting_id IS NULL AND r.brief_key=?1)
        ORDER BY m.created_at DESC LIMIT 100")
        .bind(key).fetch_all(pool).await
}

#[cfg(test)]
mod attempt_tests {
    use super::*;
    #[tokio::test]
    async fn corrections_survive_review_and_remain_specific_to_one_attempt() {
        let pool = crate::database::test_pool().await;
        for id in ["frank", "jake", "legacy", "other"] {
            sqlx::query("INSERT INTO meetings(id,title,status) VALUES(?,?,'stopped')")
                .bind(id)
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
        }
        for id in ["frank", "jake"] {
            sqlx::query("INSERT INTO call_destinations(meeting_id,task_key,name,number) VALUES(?,'task:a',?,?)").bind(id).bind(id).bind(id).execute(&pool).await.unwrap();
        }
        sqlx::query(
            "UPDATE call_destinations SET manual_outcome='voicemail_left' WHERE meeting_id='frank'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO meeting_reviews(meeting_id,revision,outcomes,brief_key) VALUES('frank','end:1','[]','task:a'),('legacy','end:2','[]','task:a')").execute(&pool).await.unwrap();
        sqlx::query("UPDATE meetings SET final_status='failed' WHERE id='jake'")
            .execute(&pool)
            .await
            .unwrap();
        let rows = attempts_for_key(&pool, "task:a").await.unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .find(|r| r.meeting_id == "frank")
                .unwrap()
                .manual_outcome
                .as_deref(),
            Some("voicemail_left")
        );
        let jake = rows.iter().find(|r| r.meeting_id == "jake").unwrap();
        assert!(jake.reviewed && jake.manual_outcome.is_none());
        assert!(
            !rows
                .iter()
                .find(|r| r.meeting_id == "legacy")
                .unwrap()
                .editable
        );
    }
}
