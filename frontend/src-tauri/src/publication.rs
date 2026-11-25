//! Durable delivery through the CICA site's existing recording upload queue.
//! The website produces its transcript and generated minutes from the original audio.
use crate::{database::repositories::meeting::MeetingsRepository, state::AppState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const SITE: &str = "https://cica.grassinside.com";
const PART_SIZE: usize = 10 * 1024 * 1024;
static BUSY: AtomicBool = AtomicBool::new(false);
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}
pub fn is_busy() -> bool {
    BUSY.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Publication {
    pub meeting_id: String,
    pub state: String,
    pub upload_id: Option<String>,
    pub bytes_sent: i64,
    pub total_bytes: i64,
    pub error: Option<String>,
}
#[derive(Serialize)]
pub struct Status {
    #[serde(flatten)]
    pub publication: Publication,
    pub url: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Part {
    etag: String,
    part_number: u64,
}

#[tauri::command]
pub fn publication_default(app: AppHandle) -> Result<bool, String> {
    let store = app
        .store("publication-settings.json")
        .map_err(|e| e.to_string())?;
    Ok(store
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true))
}
#[tauri::command]
pub fn publication_set_default(app: AppHandle, enabled: bool) -> Result<(), String> {
    let store = app
        .store("publication-settings.json")
        .map_err(|e| e.to_string())?;
    store.set("enabled", enabled);
    store.save().map_err(|e| e.to_string())
}
/// Published and filed meetings already receive notes from the portal.
pub async fn needs_local_notes(pool: &SqlitePool, meeting_id: &str) -> bool {
    sqlx::query_scalar::<_,i64>("SELECT NOT EXISTS(SELECT 1 FROM meeting_publications WHERE meeting_id=? AND state NOT IN ('failed','held'))").bind(meeting_id).fetch_one(pool).await.unwrap_or(1)!=0
}

pub async fn enqueue(pool: &SqlitePool, meeting_id: &str) -> Result<(), String> {
    sqlx::query("INSERT INTO meeting_publications (meeting_id) VALUES (?) ON CONFLICT(meeting_id) DO NOTHING")
        .bind(meeting_id).execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
pub async fn publication_status(
    app: AppHandle,
    meeting_id: String,
) -> Result<Option<Status>, String> {
    let pool = app.state::<AppState>().db_manager.pool().clone();
    let row = get(&pool, &meeting_id).await?;
    Ok(row.map(|publication| Status {
        url: publication
            .upload_id
            .as_ref()
            .map(|id| format!("{SITE}/#meetings/{id}")),
        publication,
    }))
}
#[tauri::command]
pub async fn publication_retry(app: AppHandle, meeting_id: String) -> Result<(), String> {
    let pool = app.state::<AppState>().db_manager.pool().clone();
    retry_publication(&pool, &meeting_id).await
}

async fn retry_publication(pool: &SqlitePool, meeting_id: &str) -> Result<(), String> {
    sqlx::query("UPDATE meeting_publications SET state=CASE WHEN state='held' THEN 'approved' ELSE 'queued' END, error=NULL, attempts=0, retry_at=0 WHERE meeting_id=? AND state IN ('failed','held')")
        .bind(meeting_id).execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
pub async fn publication_open(app: AppHandle, meeting_id: String) -> Result<(), String> {
    let pool = app.state::<AppState>().db_manager.pool().clone();
    let publication = get(&pool, &meeting_id)
        .await?
        .ok_or("Meeting has a local recording.")?;
    let url = match (publication.state.as_str(), publication.upload_id) {
        ("published" | "linked" | "partial", Some(id))
            if id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') =>
        {
            format!("{SITE}/#meetings/{id}")
        }
        _ => format!("{SITE}/#meetings"),
    };
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err("Open the website from the Windows app.".into())
    }
    #[cfg(windows)]
    Ok(())
}
async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Publication>, String> {
    sqlx::query_as("SELECT meeting_id,state,upload_id,bytes_sent,total_bytes,error FROM meeting_publications WHERE meeting_id=?")
        .bind(id).fetch_optional(pool).await.map_err(|e| e.to_string())
}
async fn state(pool: &SqlitePool, id: &str, value: &str) -> Result<(), String> {
    sqlx::query("UPDATE meeting_publications SET state=?, error=NULL WHERE meeting_id=?")
        .bind(value)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())
}
async fn checked(request: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = request.send().await.map_err(|_| {
        "Website connection interrupted. The saved recording is ready to retry.".to_string()
    })?;
    if !response.status().is_success() {
        return Err(format!(
            "Website returned HTTP {}. Upload will retry.",
            response.status().as_u16()
        ));
    }
    response
        .json()
        .await
        .map_err(|_| "Website returned an unreadable response.".into())
}
/// Hydrate the local editor from an already-published meeting. Local edits and concurrent
/// generations own their row and always win the insert race.
pub async fn import_published_notes(pool: &SqlitePool, meeting_id: &str) -> Result<(), String> {
    let upload: Option<String> = sqlx::query_scalar(
        "UPDATE meeting_publications SET notes_check_at=unixepoch()+120
         WHERE meeting_id=? AND state IN ('published','linked') AND upload_id IS NOT NULL AND notes_check_at<=unixepoch()
         AND NOT EXISTS (SELECT 1 FROM summary_processes s WHERE s.meeting_id=meeting_publications.meeting_id
             AND (coalesce(json_extract(s.metadata,'$.publication_id'),'')!=upload_id OR lower(s.status)!='completed'))
         RETURNING upload_id",
    ).bind(meeting_id).fetch_optional(pool).await.map_err(|e|e.to_string())?;
    let Some(id) = upload else { return Ok(()) };
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Invalid publication reference".into());
    }
    let record = checked(client()?.get(format!("{SITE}/api/meetings/{id}"))).await?;
    if record["id"].as_str() != Some(id.as_str()) {
        return Err("Website returned a different meeting".into());
    }
    let markdown = published_markdown(&record);
    if !markdown.is_empty() {
        let now = crate::database::now_rfc3339();
        sqlx::query(
            "INSERT INTO summary_processes(meeting_id,status,created_at,updated_at,result,metadata)
          VALUES (?,'completed',?,?,?,?) ON CONFLICT(meeting_id) DO UPDATE SET updated_at=excluded.updated_at,result=excluded.result,metadata=excluded.metadata WHERE json_extract(summary_processes.metadata,'$.publication_id')=json_extract(excluded.metadata,'$.publication_id') AND lower(summary_processes.status)='completed' AND summary_processes.result!=excluded.result",
        )
        .bind(meeting_id)
        .bind(&now)
        .bind(&now)
        .bind(json!({"markdown":markdown}).to_string())
        .bind(json!({"publication_id":id,"minutes_source":record["minutesSource"]}).to_string())
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }
    sqlx::query(
        "UPDATE meeting_publications SET notes_check_at=unixepoch()+600 WHERE meeting_id=?",
    )
    .bind(meeting_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn published_markdown(record: &Value) -> String {
    let mut sections = Vec::new();
    for topic in record["topics"].as_array().into_iter().flatten() {
        let mut lines = Vec::new();
        if let Some(title) = topic["title"].as_str() {
            lines.push(format!("## {title}"));
        }
        for paragraph in topic["paragraphs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            lines.push(paragraph.to_owned());
        }
        for heading in topic["headings"].as_array().into_iter().flatten() {
            if let Some(title) = heading["title"].as_str() {
                lines.push(format!("### {title}"));
            }
            lines.extend(
                heading["paragraphs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned),
            );
        }
        if !lines.is_empty() {
            sections.push(lines.join("\n\n"));
        }
    }
    let mut actions = Vec::new();
    for action in record["actions"].as_array().into_iter().flatten() {
        if let Some(task) = action["action"].as_str() {
            let mut fields = vec![task];
            fields.extend(
                ["owner", "timing"]
                    .iter()
                    .filter_map(|k| action[*k].as_str())
                    .filter(|s| !s.is_empty()),
            );
            actions.push(format!("- {}", fields.join(" · ")));
        }
    }
    if !actions.is_empty() {
        sections.push(format!("## Action items\n\n{}", actions.join("\n")));
    }
    sections.join("\n\n")
}

async fn remote_status(
    http: &reqwest::Client,
    site: &str,
    id: &str,
) -> Result<Option<&'static str>, String> {
    let response = http
        .get(format!("{site}/api/meetings/{id}"))
        .send()
        .await
        .map_err(|_| "Website status check failed.".to_string())?;
    if response.status().is_success() {
        let record: Value = response
            .json()
            .await
            .map_err(|_| "Website meeting response is unreadable.".to_string())?;
        if record["recordings"]
            .as_array()
            .is_some_and(|r| !r.is_empty())
            && record["transcript"].is_object()
            && record["minutesSource"]
                .as_str()
                .is_some_and(|s| s != "none")
        {
            return Ok(Some("published"));
        }
        return Ok(Some("processing"));
    }
    if response.status() != reqwest::StatusCode::NOT_FOUND {
        return Err("Website status check failed. Upload identity is preserved for retry.".into());
    }
    let index = checked(http.get(format!("{site}/api/meetings"))).await?;
    Ok(index["pending"]
        .as_array()
        .is_some_and(|items| items.iter().any(|x| x["id"] == id))
        .then_some("processing"))
}

fn needs_overrun_review(start: &str, scheduled_end: &str, seconds: f64) -> bool {
    let (Ok(start), Ok(end)) = (
        chrono::DateTime::parse_from_rfc3339(start),
        chrono::DateTime::parse_from_rfc3339(scheduled_end),
    ) else {
        return false;
    };
    seconds >= (end - start).num_seconds().max(0) as f64 + 900.0
}

async fn upload(pool: &SqlitePool, row: &Publication, site: &str) -> Result<(), String> {
    let http = client()?;
    if let Some(id) = &row.upload_id {
        if let Some(status) = remote_status(&http, site, id).await? {
            return state(pool, &row.meeting_id, status).await;
        }
        if row.state == "processing" {
            return Err(
                "The website is processing this recording. Check its meetings page.".into(),
            );
        }
    }
    let meeting = MeetingsRepository::get(pool, &row.meeting_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Meeting was removed.")?;
    let folder = meeting
        .folder_path
        .as_ref()
        .ok_or("Recording folder is unavailable.")?;
    if row.state == "queued" && row.upload_id.is_none() {
        if let (Some(start), Some(seconds), Some(event)) = (
            meeting.created_at.as_deref(),
            meeting.duration_seconds,
            crate::recording_context::RecordingContext::load(Path::new(folder))
                .and_then(|ctx| ctx.calendar_event),
        ) {
            if needs_overrun_review(start, &event.end, seconds) {
                return state(pool, &row.meeting_id, "held").await;
            }
        }
    }
    let mut audio = tokio::fs::File::open(Path::new(folder).join("audio.mp4"))
        .await
        .map_err(|_| {
            "The saved audio is unavailable. Open the recording folder to check it.".to_string()
        })?;
    let size = audio.metadata().await.map_err(|e| e.to_string())?.len();
    if size == 0 || size > 4 * 1024 * 1024 * 1024 {
        return Err("Website uploads support recordings between 1 byte and 4 GiB.".into());
    }
    if row.upload_id.is_some() && row.total_bytes != size as i64 {
        return Err(
            "The recording size changed. Preserve the original audio before resuming this upload."
                .into(),
        );
    }
    let id = if let Some(id) = &row.upload_id {
        id.clone()
    } else {
        let created = chrono::DateTime::parse_from_rfc3339(
            meeting
                .created_at
                .as_deref()
                .ok_or("Meeting date needs attention.")?,
        )
        .map_err(|_| "Meeting date needs attention.".to_string())?;
        // The current calendar and website both use Arizona time (UTC-7 year-round).
        let local = created
            .with_timezone(&chrono::FixedOffset::west_opt(7 * 3600).expect("Arizona offset"));
        let response = checked(http.post(format!("{site}/api/uploads")).header("Origin", site).json(&json!({
            "category":"internal", "company":"Cica-Huntek", "title":meeting.title.chars().take(200).collect::<String>(),
            "date":local.format("%Y-%m-%d").to_string(), "time":local.format("%I:%M %p").to_string(),
            "filename":"audio.mp4", "contentType":"audio/mp4", "size":size,
        }))).await?;
        let id = response["id"]
            .as_str()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 120
                    && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
            .ok_or("Website returned an invalid upload identity.")?
            .to_string();
        if response["partSize"].as_u64() != Some(PART_SIZE as u64) {
            return Err("Website upload protocol changed. Update Meetily before retrying.".into());
        }
        sqlx::query("UPDATE meeting_publications SET upload_id=?,total_bytes=?,state='uploading' WHERE meeting_id=?")
            .bind(&id).bind(size as i64).bind(&row.meeting_id).execute(pool).await.map_err(|e| e.to_string())?;
        id
    };
    state(pool, &row.meeting_id, "uploading").await?;
    let saved: String =
        sqlx::query_scalar("SELECT parts_json FROM meeting_publications WHERE meeting_id=?")
            .bind(&row.meeting_id)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
    let mut parts: Vec<Part> = serde_json::from_str(&saved)
        .map_err(|_| "Upload checkpoint needs attention.".to_string())?;
    let mut sent = (parts.len() * PART_SIZE) as u64;
    if sent > size {
        sent = size;
    }
    audio
        .seek(std::io::SeekFrom::Start(sent))
        .await
        .map_err(|e| e.to_string())?;
    while sent < size {
        let mut bytes = vec![0; (size - sent).min(PART_SIZE as u64) as usize];
        audio.read_exact(&mut bytes).await.map_err(|_| {
            "Audio changed during upload. Retry from the saved original.".to_string()
        })?;
        let length = bytes.len() as u64;
        let number = parts.len() as u64 + 1;
        let response = http
            .put(format!("{site}/api/uploads/{id}/parts/{number}"))
            .header("Origin", site)
            .body(bytes)
            .send()
            .await
            .map_err(|_| "Upload connection interrupted. Saved parts will resume.".to_string())?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            // remote_status above confirmed this ID is absent from the published and pending lists.
            // The site's open-upload receipt expires after two days; start a fresh upload next time.
            sqlx::query("UPDATE meeting_publications SET upload_id=NULL,parts_json='[]',bytes_sent=0,total_bytes=0 WHERE meeting_id=?")
                .bind(&row.meeting_id).execute(pool).await.map_err(|e| e.to_string())?;
            return Err(
                "The upload session expired. A fresh upload will start automatically.".into(),
            );
        }
        if !response.status().is_success() {
            return Err(format!(
                "Website returned HTTP {}. Upload will retry.",
                response.status().as_u16()
            ));
        }
        let value = response
            .json::<Value>()
            .await
            .map_err(|_| "Website part receipt is unreadable.".to_string())?;
        let part: Part = serde_json::from_value(value)
            .map_err(|_| "Website part receipt is unreadable.".to_string())?;
        if part.part_number != number {
            return Err("Website returned an unexpected part receipt.".into());
        }
        parts.push(part);
        sent += length;
        sqlx::query("UPDATE meeting_publications SET parts_json=?,bytes_sent=? WHERE meeting_id=?")
            .bind(serde_json::to_string(&parts).map_err(|e| e.to_string())?)
            .bind(sent as i64)
            .bind(&row.meeting_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    checked(
        http.post(format!("{site}/api/uploads/{id}/complete"))
            .header("Origin", site)
            .json(&json!({"parts":parts})),
    )
    .await?;
    state(pool, &row.meeting_id, "processing").await
}

/// Runs on startup and every 20 seconds. Only opt-in rows with saved audio are eligible.
pub async fn tick(pool: &SqlitePool) -> Result<(), String> {
    tick_at(pool, SITE).await
}
#[derive(sqlx::FromRow)]
struct Due {
    #[sqlx(flatten)]
    publication: Publication,
    folder_path: Option<String>,
}
/// The meeting's audio is still being encoded (or waits for the next start): not a failure.
/// A save registered in this process decides; a journal counts only for meetings it has not
/// registered, so a journal left beside finished audio does not hold the upload.
fn audio_pending(due: &Due) -> bool {
    let meeting_id = &due.publication.meeting_id;
    match due.folder_path.as_deref() {
        Some(folder) => crate::audio::audio_save::is_pending(meeting_id, Path::new(folder)),
        None => crate::audio::audio_save::is_saving(meeting_id),
    }
}
async fn tick_at(pool: &SqlitePool, site: &str) -> Result<(), String> {
    if BUSY
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }
    let _guard = Guard;
    let rows: Vec<Due> = sqlx::query_as("SELECT p.meeting_id,p.state,p.upload_id,p.bytes_sent,p.total_bytes,p.error,m.folder_path FROM meeting_publications p JOIN meetings m ON m.id=p.meeting_id WHERE m.status IN ('stopped','interrupted') AND p.state IN ('queued','approved','uploading','processing') AND p.retry_at <= ? ORDER BY (p.state='processing'),m.created_at LIMIT 10")
        .bind(chrono::Utc::now().timestamp()).fetch_all(pool).await.map_err(|e| e.to_string())?;
    for due in rows {
        if audio_pending(&due) {
            continue;
        }
        let row = due.publication;
        if let Err(error) = upload(pool, &row, site).await {
            sqlx::query("UPDATE meeting_publications SET attempts=attempts+1, state=CASE WHEN state='processing' THEN 'processing' WHEN attempts>=4 THEN 'failed' WHEN state='approved' THEN 'approved' ELSE 'queued' END,error=?,retry_at=? WHERE meeting_id=?")
                .bind(error).bind(chrono::Utc::now().timestamp() + 60).bind(&row.meeting_id).execute(pool).await.map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
pub fn start(app: &AppHandle) {
    let pool = app.state::<AppState>().db_manager.pool().clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if let Err(error) = tick(&pool).await {
                log::warn!("Publication queue: {error}");
            }
            let due: Vec<String> = sqlx::query_scalar("SELECT p.meeting_id FROM meeting_publications p LEFT JOIN summary_processes s ON s.meeting_id=p.meeting_id WHERE p.state IN ('published','linked') AND p.notes_check_at<=unixepoch() AND (s.meeting_id IS NULL OR json_extract(s.metadata,'$.publication_id')=p.upload_id AND lower(s.status)='completed') ORDER BY p.notes_check_at LIMIT 2").fetch_all(&pool).await.unwrap_or_default();
            for id in due {
                if let Err(error) = import_published_notes(&pool, &id).await {
                    log::warn!("Published minutes readback: {error}");
                }
            }
            tokio::time::sleep(Duration::from_secs(20)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_calendar_overruns_at_fifteen_minutes() {
        let start = "2026-11-25T11:30:00-07:00";
        let end = "2026-11-25T19:00:00Z";
        assert!(!needs_overrun_review(start, end, 2699.0));
        assert!(needs_overrun_review(start, end, 2700.0));
        assert!(needs_overrun_review(start, end, 10383.0));
    }

    use crate::database::repositories::meeting::NewMeeting;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    #[test]
    fn minutes_and_action_owners_survive_readback() {
        let record = json!({"topics":[{"title":"Procurement","paragraphs":["Request a quote."]}],"actions":[{"owner":"Chi","action":"Send drawing","timing":"Monday"}]});
        assert_eq!(published_markdown(&record),"## Procurement\n\nRequest a quote.\n\n## Action items\n\n- Send drawing · Chi · Monday");
        assert!(published_markdown(&json!({"topics":[],"actions":[]})).is_empty());
    }
    #[tokio::test]
    async fn consent_checkpoint_resume_and_lost_completion_preserve_one_upload() {
        // As `setup` does; without it the test depends on another test building a client first.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let pool = crate::database::test_pool().await;
        let dir = tempfile::tempdir().unwrap();
        let audio = vec![0x42u8; PART_SIZE + 5];
        tokio::fs::write(dir.path().join("audio.mp4"), &audio)
            .await
            .unwrap();
        MeetingsRepository::create(
            &pool,
            &NewMeeting {
                id: "meeting",
                title: "Test meeting",
                folder_path: dir.path().to_str().unwrap(),
                status: "stopped",
            },
        )
        .await
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let site = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let completed = Arc::new(AtomicBool::new(false));
        let published = Arc::new(AtomicBool::new(false));
        let is_published = published.clone();
        let server = tokio::spawn(async move {
            let mut second_attempts = 0;
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = vec![];
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(socket.read_u8().await.unwrap());
                }
                let header = String::from_utf8(header).unwrap();
                let line = header.lines().next().unwrap().to_string();
                let length = header
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_string)
                    })
                    .map(|n| n.parse::<usize>().unwrap())
                    .unwrap_or(0);
                let mut body = vec![0; length];
                socket.read_exact(&mut body).await.unwrap();
                observed.lock().unwrap().push(line.clone());
                let (code, response) = if line.starts_with("POST /api/uploads ") {
                    let body: Value = serde_json::from_slice(&body).unwrap();
                    assert_eq!(body["size"], PART_SIZE + 5);
                    assert_eq!(body["title"], "Test meeting");
                    assert!(header.to_lowercase().contains("origin: http://127.0.0.1:"));
                    (200, json!({"id":"test-upload", "partSize":PART_SIZE}))
                } else if line.starts_with("PUT ") {
                    let number = if line.contains("/parts/1 ") { 1 } else { 2 };
                    assert_eq!(body, vec![0x42; if number == 1 { PART_SIZE } else { 5 }]);
                    if number == 2 {
                        second_attempts += 1;
                    }
                    if number == 2 && second_attempts == 1 {
                        (500, json!({}))
                    } else {
                        (
                            200,
                            json!({"etag":format!("part-{number}"),"partNumber":number}),
                        )
                    }
                } else if line.starts_with("POST ") && line.contains("/complete ") {
                    assert_eq!(
                        serde_json::from_slice::<Value>(&body).unwrap()["parts"]
                            .as_array()
                            .unwrap()
                            .len(),
                        2
                    );
                    completed.store(true, Ordering::SeqCst);
                    (500, json!({})) // Server completed the upload; client lost the receipt.
                } else if line.starts_with("GET /api/meetings/test-upload ") {
                    if is_published.load(Ordering::SeqCst) {
                        (
                            200,
                            json!({"transcript":{},"minutesSource":"generated","recordings":[{"label":"Audio"}]}),
                        )
                    } else {
                        (404, json!({}))
                    }
                } else if line.starts_with("GET /api/meetings ") {
                    (
                        200,
                        json!({"pending": if completed.load(Ordering::SeqCst) { vec![json!({"id":"test-upload"})] } else { vec![] }}),
                    )
                } else {
                    panic!("Unexpected request: {line}");
                };
                use tokio::io::AsyncWriteExt;
                let body = response.to_string();
                socket.write_all(format!("HTTP/1.1 {code} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
        });
        tick_at(&pool, &site).await.unwrap(); // Historical meetings have no publication row.
        assert!(requests.lock().unwrap().is_empty());
        enqueue(&pool, "meeting").await.unwrap();
        enqueue(&pool, "meeting").await.unwrap();
        sqlx::query("UPDATE meetings SET status='recording'")
            .execute(&pool)
            .await
            .unwrap();
        tick_at(&pool, &site).await.unwrap();
        assert!(requests.lock().unwrap().is_empty());
        sqlx::query("UPDATE meetings SET status='stopped'")
            .execute(&pool)
            .await
            .unwrap();
        // Stopped with the audio still in its journal, then still encoding: the row waits
        // without a request, an attempt or an error.
        let mut journal = crate::audio::journal::AudioJournal::create(dir.path(), 48000).unwrap();
        journal
            .add_chunk(crate::audio::AudioChunk {
                data: vec![0.1; 480],
                device_type: crate::audio::recording_state::DeviceType::Microphone,
            })
            .unwrap();
        journal.close().unwrap();
        tick_at(&pool, &site).await.unwrap();
        std::fs::remove_dir_all(dir.path().join(crate::audio::journal::CHECKPOINTS_DIR)).unwrap();
        crate::audio::audio_save::register("meeting");
        tick_at(&pool, &site).await.unwrap();
        assert!(requests.lock().unwrap().is_empty());
        let waiting: (i64, Option<String>, String) = sqlx::query_as(
            "SELECT attempts,error,state FROM meeting_publications WHERE meeting_id='meeting'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(waiting, (0, None, "queued".to_string()));
        crate::audio::audio_save::finish("meeting", Ok(dir.path().join("audio.mp4")));
        std::fs::write(dir.path().join("recording-context.json"), serde_json::to_vec(&json!({
            "mode":"online","calendar_event":{"id":"event","title":"Demo","start":"2026-11-25T18:30:00Z","end":"2026-11-25T19:00:00Z","online":true,"location":"Teams","organizer":null,"invitees":[]}
        })).unwrap()).unwrap();
        sqlx::query("UPDATE meetings SET created_at='2026-11-25T18:30:00Z',duration_seconds=10383 WHERE id='meeting'").execute(&pool).await.unwrap();
        tick_at(&pool, &site).await.unwrap();
        tick_at(&pool, &site).await.unwrap();
        assert_eq!(get(&pool, "meeting").await.unwrap().unwrap().state, "held");
        assert!(requests.lock().unwrap().is_empty());
        retry_publication(&pool, "meeting").await.unwrap();

        tick_at(&pool, &site).await.unwrap();
        assert_eq!(
            get(&pool, "meeting").await.unwrap().unwrap().bytes_sent,
            PART_SIZE as i64
        );
        for _ in 0..2 {
            sqlx::query("UPDATE meeting_publications SET retry_at=0")
                .execute(&pool)
                .await
                .unwrap();
            tick_at(&pool, &site).await.unwrap();
        }
        assert_eq!(
            get(&pool, "meeting").await.unwrap().unwrap().state,
            "processing"
        );
        published.store(true, Ordering::SeqCst);
        tick_at(&pool, &site).await.unwrap();
        assert_eq!(
            get(&pool, "meeting").await.unwrap().unwrap().state,
            "published"
        );
        assert_eq!(
            tokio::fs::read(dir.path().join("audio.mp4")).await.unwrap(),
            audio
        );
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.starts_with("POST /api/uploads "))
                .count(),
            1
        );
        assert_eq!(
            requests.iter().filter(|r| r.contains("/parts/1 ")).count(),
            1
        );
        assert_eq!(
            requests.iter().filter(|r| r.contains("/parts/2 ")).count(),
            2
        );
        assert_eq!(
            requests.iter().filter(|r| r.contains("/complete ")).count(),
            1
        );
        server.abort();
    }
}
