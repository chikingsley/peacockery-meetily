//! Per-user Outlook Web session, using the same REST route as the GMK collector.
use crate::recording_context::{CalendarEvent, CalendarPerson, CalendarSnapshot};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
#[cfg(windows)]
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::Manager;

const ROOT: &str = "https://outlook.office.com/api/v2.0";

static RENEW_AFTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
static CONNECTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const OUTLOOK_BUSY: &str = "Meetily is checking Outlook. Try again when the check finishes.";
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        CONNECTING.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}
#[derive(Serialize, Deserialize)]
struct Auth {
    token: String,
    account: String,
}
/// `outlook-calendar.json`; an empty calendar means none is selected.
#[derive(Default, Serialize, Deserialize)]
struct Selection {
    calendar: String,
}
#[derive(Serialize)]
pub struct Connection {
    pub account: Option<String>,
    pub calendars: Vec<Value>,
    pub selected: String,
    pub error: Option<String>,
}
fn auth_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("outlook-session.bin"))
}
fn read_auth(app: &tauri::AppHandle) -> Result<Option<Auth>, String> {
    let path = auth_path(app)?;
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Windows session storage needs attention. Connect Outlook.".into()),
    };
    serde_json::from_slice(&crate::outlook_secret::protect(&bytes, false)?)
        .map(Some)
        .map_err(|_| "Connect Outlook to renew calendar access.".into())
}
#[cfg(windows)]
fn save_auth(app: &tauri::AppHandle, auth: &Auth) -> Result<(), String> {
    use std::io::Write;
    let path = auth_path(app)?;
    let sealed = crate::outlook_secret::protect(
        &serde_json::to_vec(auth).map_err(|e| e.to_string())?,
        true,
    )?;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing session folder.")?)
        .map_err(|e| e.to_string())?;
    temp.write_all(&sealed).map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path)
        .map_err(|_| "Could not save the Outlook session. Connect Outlook.".to_string())?;
    Ok(())
}
async fn current_auth(app: &tauri::AppHandle) -> Result<Option<Auth>, String> {
    let Some(auth) = read_auth(app)? else {
        return Ok(None);
    };
    if token_identity(&auth.token).is_some() {
        return Ok(Some(auth));
    }
    // A saved browser session renews expiring access automatically. A provider
    // challenge returns an explicit reconnect prompt instead of another account.
    if chrono::Utc::now().timestamp() < RENEW_AFTER.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("Meetily could not renew calendar access automatically.".into());
    }
    if let Err(error) = capture(app, false).await {
        RENEW_AFTER.store(
            chrono::Utc::now().timestamp() + 300,
            std::sync::atomic::Ordering::SeqCst,
        );
        return Err(error);
    }
    read_auth(app)
}
fn selection_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("outlook-calendar.json"))
}
fn selection(app: &tauri::AppHandle) -> Selection {
    selection_path(app)
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .map(|bytes| parse_selection(&bytes))
        .unwrap_or_default()
}
fn parse_selection(bytes: &[u8]) -> Selection {
    serde_json::from_slice(bytes).unwrap_or_default()
}
async fn get(auth: &Auth, path: &str) -> Result<Value, String> {
    let url = if path.starts_with("https://outlook.office.com/api/v2.0/") {
        path.to_string()
    } else {
        format!("{ROOT}{path}")
    };
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .bearer_auth(&auth.token)
        .header("x-anchormailbox", &auth.account)
        .header("Prefer", "outlook.timezone=\"UTC\"")
        .send()
        .await
        .map_err(|_| "Calendar connection failed. Try again.".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Outlook returned HTTP {} when reading the calendar.",
            response.status().as_u16()
        ));
    }
    response
        .json()
        .await
        .map_err(|_| "Outlook returned an unreadable calendar response.".into())
}
fn token_identity(token: &str) -> Option<String> {
    let part = token.split('.').nth(1)?;
    let p: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).ok()?).ok()?;
    if !["https://outlook.office.com", "https://outlook.office.com/"].contains(&p["aud"].as_str()?)
    {
        return None;
    }
    if p["exp"].as_i64()? < chrono::Utc::now().timestamp() + 300 {
        return None;
    }
    ["preferred_username", "upn", "unique_name"]
        .iter()
        .find_map(|k| p[*k].as_str().map(str::to_string))
}
async fn calendars(auth: &Auth) -> Result<Vec<Value>, String> {
    let mut path = "/me/calendars?$top=100".to_string();
    let mut result = vec![];
    for _ in 0..20 {
        let v = get(auth, &path).await?;
        let page = v["value"]
            .as_array()
            .ok_or("Outlook calendar list is incomplete.")?;
        result.extend(
            page.iter()
                .map(|c| json!({"id":c["Id"],"name":c["Name"],"owner":c["Owner"]["Address"]})),
        );
        match v["@odata.nextLink"].as_str() {
            Some(next) if next.starts_with("https://outlook.office.com/api/v2.0/") => {
                path = next.into()
            }
            None => return Ok(result),
            _ => return Err("Unexpected calendar pagination.".into()),
        }
    }
    Err("Calendar list exceeded its page limit.".into())
}
async fn wait_for_calendar_check(busy: &std::sync::atomic::AtomicBool) {
    while busy.load(std::sync::atomic::Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
#[tauri::command]
pub async fn outlook_status(app: tauri::AppHandle) -> Result<Connection, String> {
    wait_for_calendar_check(&CONNECTING).await;
    let s = selection(&app);
    let previous = read_auth(&app).ok().flatten();
    let auth = match current_auth(&app).await {
        Ok(auth) => auth,
        Err(error) => {
            return Ok(Connection {
                account: previous.map(|a| a.account),
                calendars: vec![],
                selected: s.calendar,
                error: Some(error),
            })
        }
    };
    let Some(auth) = auth else {
        return Ok(Connection {
            account: None,
            calendars: vec![],
            selected: s.calendar,
            error: None,
        });
    };
    match calendars(&auth).await {
        Ok(list) => Ok(Connection {
            account: Some(auth.account),
            calendars: list,
            selected: s.calendar,
            error: None,
        }),
        Err(error) => Ok(Connection {
            account: Some(auth.account),
            calendars: vec![],
            selected: s.calendar,
            error: Some(error),
        }),
    }
}
#[tauri::command]
pub async fn outlook_select(app: tauri::AppHandle, calendar: String) -> Result<(), String> {
    let auth = current_auth(&app).await?.ok_or("Connect Outlook first.")?;
    if !calendars(&auth)
        .await?
        .iter()
        .any(|c| c["id"].as_str() == Some(&calendar))
    {
        return Err("Choose a calendar from your connected account.".into());
    }
    std::fs::write(
        selection_path(&app)?,
        serde_json::to_vec(&Selection { calendar }).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn outlook_disconnect(app: tauri::AppHandle) -> Result<(), String> {
    if CONNECTING.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("Finish Outlook sign-in before disconnecting.".into());
    }
    std::fs::write(
        selection_path(&app)?,
        serde_json::to_vec(&Selection::default()).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    match std::fs::remove_file(auth_path(&app)?) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.to_string()),
    }
    let profile = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("outlook-browser");
    if profile.exists() {
        std::fs::remove_dir_all(profile)
            .map_err(|_| "Close the Meetily Outlook window, then disconnect again.".to_string())?;
    }
    Ok(())
}
#[tauri::command]
pub async fn outlook_connect(app: tauri::AppHandle) -> Result<Connection, String> {
    let result = capture(&app, true).await;
    if result.is_ok() {
        RENEW_AFTER.store(0, std::sync::atomic::Ordering::SeqCst);
    }
    result
}
async fn capture(app: &tauri::AppHandle, interactive: bool) -> Result<Connection, String> {
    use std::sync::atomic::Ordering;
    CONNECTING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| OUTLOOK_BUSY.to_string())?;
    let _guard = Guard;
    #[cfg(not(windows))]
    {
        let _ = (app, interactive);
        Err("Connect Outlook is available in the Windows app.".into())
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        for hive in ["HKCU", "HKLM"] {
            {
                let (policy, blocked) = ("RemoteDebuggingAllowed", "0x0");
                let output = std::process::Command::new("reg.exe")
                    .args([
                        "query",
                        &format!("{hive}\\SOFTWARE\\Policies\\Microsoft\\Edge"),
                        "/v",
                        policy,
                    ])
                    .creation_flags(0x08000000)
                    .output();
                if output.ok().filter(|o| o.status.success()).is_some_and(|o| {
                    String::from_utf8_lossy(&o.stdout).lines().any(|line| {
                        let words: Vec<_> = line.split_whitespace().collect();
                        words.first() == Some(&policy) && words.last() == Some(&blocked)
                    })
                }) {
                    return Err("Your organization’s Edge policy blocks the calendar connection. Your IT administrator can review the Meetily Outlook connection; recording remains available.".into());
                }
            }
        }
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("outlook-browser");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let edge = [
            std::env::var("PROGRAMFILES(X86)").unwrap_or_default(),
            std::env::var("PROGRAMFILES").unwrap_or_default(),
            std::env::var("LOCALAPPDATA").unwrap_or_default(),
        ]
        .iter()
        .map(|p| std::path::PathBuf::from(p).join("Microsoft/Edge/Application/msedge.exe"))
        .find(|p| p.exists())
        .ok_or("Install Microsoft Edge to connect Outlook.")?;
        let mut command = std::process::Command::new(edge);
        command.args([
            "--remote-debugging-port=0",
            "--remote-debugging-address=127.0.0.1",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-mode",
            "--disable-sync",
            "--disable-extensions",
            "--disable-features=msImplicitSignin",
            "--disable-background-timer-throttling",
            "--disable-renderer-backgrounding",
            "--disable-backgrounding-occluded-windows",
            "--new-window",
        ]);
        if !interactive {
            command.arg("--headless=new");
        }
        let mut child = command
            .arg(format!("--user-data-dir={}", dir.display()))
            .arg("https://outlook.office365.com/cicahuntek.us")
            .creation_flags(0x08000000)
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut browser_endpoint = None;
        let task = async {
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .map_err(|e| e.to_string())?;
            let (port, endpoint) = loop {
                if let Ok(text) = std::fs::read_to_string(dir.join("DevToolsActivePort")) {
                    if let Some(port) = text.lines().next().and_then(|p| p.parse::<u16>().ok()) {
                        if let Ok(response) = http
                            .get(format!("http://127.0.0.1:{port}/json/list"))
                            .send()
                            .await
                        {
                            if let Ok(tabs) = response.json::<Vec<Value>>().await {
                                if let Some(path) = text.lines().nth(1) {
                                    browser_endpoint = Some(format!("ws://127.0.0.1:{port}{path}"));
                                }
                                if let Some(endpoint) = tabs
                                    .iter()
                                    .find(|t| {
                                        t["type"] == "page"
                                            && t["url"].as_str().is_some_and(|u| {
                                                [
                                                    "https://outlook.office.com/",
                                                    "https://outlook.office365.com/",
                                                    "https://outlook.cloud.microsoft/",
                                                    "https://login.microsoftonline.com/",
                                                ]
                                                .iter()
                                                .any(|host| u.starts_with(host))
                                            })
                                    })
                                    .and_then(|t| t["webSocketDebuggerUrl"].as_str())
                                {
                                    break (port, endpoint.to_string());
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            };
            if !endpoint.starts_with(&format!("ws://127.0.0.1:{port}/"))
                && !endpoint.starts_with(&format!("ws://localhost:{port}/"))
            {
                return Err("Unexpected browser connection.".into());
            }
            let (mut socket, _) = tokio_tungstenite::connect_async(&endpoint)
                .await
                .map_err(|e| e.to_string())?;
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    json!({"id":1,"method":"Network.enable"}).to_string().into(),
                ))
                .await
                .map_err(|e| e.to_string())?;
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    json!({"id":2,"method":"Page.reload"}).to_string().into(),
                ))
                .await
                .map_err(|e| e.to_string())?;
            while let Some(message) = socket.next().await {
                let text = message
                    .map_err(|_| {
                        "Outlook window closed. Choose Connect Outlook to retry.".to_string()
                    })?
                    .into_text()
                    .map_err(|e| e.to_string())?;
                let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if v["method"] != "Network.requestWillBeSent"
                    && v["method"] != "Network.requestWillBeSentExtraInfo"
                {
                    continue;
                }
                let h = v
                    .pointer("/params/request/headers")
                    .or_else(|| v.pointer("/params/headers"));
                let token = h
                    .and_then(Value::as_object)
                    .and_then(|m| {
                        m.iter()
                            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                    })
                    .and_then(|(_, v)| v.as_str())
                    .and_then(|v| v.strip_prefix("Bearer "));
                if let Some(token) = token {
                    if let Some(account) = token_identity(token) {
                        let auth = Auth {
                            token: token.into(),
                            account,
                        };
                        let list = calendars(&auth).await?;
                        if !interactive
                            && read_auth(app)?
                                .is_some_and(|old| !old.account.eq_ignore_ascii_case(&auth.account))
                        {
                            return Err("The Outlook account changed. Choose Connect Outlook to confirm it.".into());
                        }
                        save_auth(app, &auth)?;
                        let previous = selection(app);
                        let chosen = list
                            .iter()
                            .find(|c| c["id"].as_str() == Some(&previous.calendar))
                            .or_else(|| {
                                list.iter().find(|c| {
                                    c["owner"]
                                        .as_str()
                                        .is_some_and(|s| s.eq_ignore_ascii_case(&auth.account))
                                })
                            })
                            .or(list.first())
                            .and_then(|c| c["id"].as_str())
                            .ok_or("Choose an Outlook account with a calendar.")?
                            .to_string();
                        std::fs::write(
                            selection_path(app)?,
                            serde_json::to_vec(&Selection {
                                calendar: chosen.clone(),
                            })
                            .map_err(|e| e.to_string())?,
                        )
                        .map_err(|e| e.to_string())?;
                        let _ = socket
                            .send(tokio_tungstenite::tungstenite::Message::Text(
                                json!({"id":3,"method":"Browser.close"}).to_string().into(),
                            ))
                            .await;
                        let _ = socket.close(None).await;
                        return Ok(Connection {
                            account: Some(auth.account),
                            calendars: list,
                            selected: chosen,
                            error: None,
                        });
                    }
                }
            }
            Err("Outlook connection ended. Choose Connect Outlook to retry.".into())
        };
        let result = tokio::time::timeout(
            Duration::from_secs(if interactive { 600 } else { 60 }),
            task,
        )
        .await
        .unwrap_or_else(|_| Err("Meetily could not renew calendar access automatically.".into()));
        // Always close only Meetily's dedicated browser, including errors and timeouts.
        if let Some(endpoint) = browser_endpoint {
            if let Ok(Ok((mut socket, _))) = tokio::time::timeout(
                Duration::from_secs(3),
                tokio_tungstenite::connect_async(endpoint),
            )
            .await
            {
                let _ = socket
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        json!({"id":9,"method":"Browser.close"}).to_string().into(),
                    ))
                    .await;
            }
        }
        for _ in 0..12 {
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        // A hung browser must release this app-owned profile so the next attempt
        // can start with the current launch settings. Only touch our child.
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        result
    }
}
fn person(v: &Value) -> CalendarPerson {
    let a = &v["EmailAddress"];
    CalendarPerson {
        name: a["Name"].as_str().unwrap_or("").into(),
        email: a["Address"].as_str().unwrap_or("").into(),
    }
}
/// The selected calendar's events from yesterday to two days ahead; `None` without a selected
/// calendar or a saved session. Disconnect clears both, so Microsoft is not contacted.
pub async fn snapshot(app: &tauri::AppHandle) -> Result<Option<CalendarSnapshot>, String> {
    let s = selection(app);
    if s.calendar.is_empty() {
        return Ok(None);
    }
    // Managed installations receive a private agenda for their explicitly selected calendar.
    if let Ok(bytes) = std::fs::read(
        app.path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("managed-agenda.json"),
    ) {
        if let Some(snapshot) = managed_snapshot(&bytes, &s.calendar, chrono::Utc::now()) {
            return Ok(Some(snapshot));
        }
    }
    let Some(auth) = current_auth(app).await? else {
        return Ok(None);
    };
    let start = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let end = (chrono::Utc::now() + chrono::Duration::days(2)).to_rfc3339();
    let id: String = url::form_urlencoded::byte_serialize(s.calendar.as_bytes()).collect();
    let mut next = format!(
        "/me/calendars/{id}/calendarview?{}",
        url::form_urlencoded::Serializer::new(String::new())
            .append_pair("startDateTime", &start)
            .append_pair("endDateTime", &end)
            .append_pair("$top", "200")
            .finish()
    );
    let mut events = vec![];
    for _ in 0..20 {
        let v = get(&auth, &next).await?;
        for e in v["value"]
            .as_array()
            .ok_or("Calendar response is incomplete.")?
        {
            if e["IsCancelled"] == true || e["IsAllDay"] == true {
                continue;
            }
            let time = |name: &str| -> Result<String, String> {
                let t = e[name]["DateTime"].as_str().ok_or("Missing event time.")?;
                if e[name]["TimeZone"] != "UTC" {
                    return Err("Expected UTC calendar times.".into());
                }
                Ok(if t.ends_with('Z') {
                    t.into()
                } else {
                    format!("{t}Z")
                })
            };
            let body = e["Body"]["Content"].as_str().unwrap_or("").to_lowercase();
            events.push(CalendarEvent {
                id: e["Id"]
                    .as_str()
                    .ok_or("Missing calendar event identity.")?
                    .into(),
                title: e["Subject"].as_str().unwrap_or("Meeting").into(),
                start: time("Start")?,
                end: time("End")?,
                location: e["Location"]["DisplayName"].as_str().unwrap_or("").into(),
                online: e["OnlineMeetingUrl"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
                    || ["teams.microsoft.com/", "zoom.us/", "meet.google.com/"]
                        .iter()
                        .any(|url| body.contains(url)),
                organizer: Some(person(&e["Organizer"])),
                invitees: e["Attendees"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter(|v| v["Type"] != "Resource")
                            .map(person)
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
        match v["@odata.nextLink"].as_str() {
            None => {
                events.sort_by(|a, b| a.start.cmp(&b.start));
                return Ok(Some(CalendarSnapshot {
                    calls: Vec::new(),
                    observed: chrono::Utc::now().to_rfc3339(),
                    events,
                }));
            }
            Some(n) if n.starts_with("https://outlook.office.com/api/v2.0/") => next = n.into(),
            _ => return Err("Unexpected calendar pagination.".into()),
        }
    }
    Err("Calendar window exceeded its page limit.".into())
}

fn managed_snapshot(
    bytes: &[u8],
    selected: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<CalendarSnapshot> {
    #[derive(Deserialize)]
    struct Agenda {
        calendar: String,
        snapshot: CalendarSnapshot,
    }
    let agenda: Agenda = serde_json::from_slice(bytes).ok()?;
    let observed = chrono::DateTime::parse_from_rfc3339(&agenda.snapshot.observed).ok()?;
    let age = now.signed_duration_since(observed).num_seconds();
    (agenda.calendar == selected && !selected.is_empty() && (0..900).contains(&age))
        .then_some(agenda.snapshot)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn settings_waits_for_an_invisible_background_check() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let busy = AtomicBool::new(true);
        let waiting = wait_for_calendar_check(&busy);
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut waiting)
                .await
                .is_err()
        );
        busy.store(false, Ordering::SeqCst);
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap();
    }

    #[test]
    fn disconnect_leaves_no_selected_calendar() {
        // Files written before the `disabled` flag was removed still load.
        assert_eq!(
            parse_selection(br#"{"calendar":"","disabled":true}"#).calendar,
            ""
        );
        assert_eq!(
            parse_selection(br#"{"calendar":"abc","disabled":false}"#).calendar,
            "abc"
        );
        let written = serde_json::to_vec(&Selection::default()).unwrap();
        assert_eq!(written, br#"{"calendar":""}"#);
        assert!(parse_selection(&written).calendar.is_empty());
        assert!(parse_selection(b"junk").calendar.is_empty());
    }

    #[test]
    fn managed_agenda_requires_selected_calendar_and_fresh_observation() {
        let now = chrono::Utc::now();
        let packet = serde_json::to_vec(
            &json!({"calendar":"mine","snapshot":{"observed":now.to_rfc3339(),"events":[]}}),
        )
        .unwrap();
        assert!(managed_snapshot(&packet, "mine", now).is_some());
        assert!(managed_snapshot(&packet, "other", now).is_none());
        assert!(managed_snapshot(&packet, "", now).is_none());
        assert!(managed_snapshot(&packet, "mine", now + chrono::Duration::minutes(16)).is_none());
    }

    fn token(audience: &str, expires: i64) -> String {
        format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(
                    &json!({"aud":audience,"exp":expires,"upn":"person@example.test"})
                )
                .unwrap()
            )
        )
    }
    #[test]
    fn accepts_only_outlook_identity_with_remaining_lifetime() {
        let future = chrono::Utc::now().timestamp() + 3600;
        assert_eq!(
            token_identity(&token("https://outlook.office.com", future)),
            Some("person@example.test".into())
        );
        assert_eq!(
            token_identity(&token("https://graph.microsoft.com", future)),
            None
        );
        assert_eq!(
            token_identity(&token("https://outlook.office.com", future - 3590)),
            None
        );
        assert_eq!(token_identity("malformed"), None);
    }
}

/// Stage an edit for the authenticated GMK agenda publisher. The PC holds no Trello credential.
#[tauri::command]
pub async fn change_call_task(
    app: tauri::AppHandle,
    task_id: String,
    due: Option<String>,
) -> Result<(), String> {
    let calendar = selection(&app).calendar;
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let bytes = std::fs::read(base.join("managed-agenda.json"))
        .map_err(|_| "Managed agenda unavailable")?;
    let task = managed_snapshot(&bytes, &calendar, chrono::Utc::now())
        .and_then(|s| s.calls.into_iter().find(|t| t.id == task_id && t.editable))
        .ok_or("Refresh the call list before editing this task")?;
    if let Some(time) = &due {
        let date =
            chrono::DateTime::parse_from_rfc3339(time).map_err(|_| "Choose a valid due time")?;
        if date <= chrono::Utc::now() {
            return Err("Choose a future due time".into());
        }
    }
    let id = uuid::Uuid::parse_str(&task.id).map_err(|_| "Invalid task ID")?;
    let dir = base.join("call-changes");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{id}.json"));
    let mut temp = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut temp, &json!({"calendar":calendar,"id":task.id,"title":task.title,"before":task.due,"due":due,"request":uuid::Uuid::new_v4().to_string()})).map_err(|e|e.to_string())?;
    temp.persist_noclobber(path)
        .map_err(|_| "A change is already queued for this task")?;
    Ok(())
}

#[tauri::command]
pub fn call_change_status(
    app: tauri::AppHandle,
    task_id: String,
) -> Result<Option<String>, String> {
    let id = uuid::Uuid::parse_str(&task_id).map_err(|_| "Invalid task ID")?;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("call-changes");
    if dir.join(format!("{id}.json")).exists() {
        return Ok(Some("Queued for Trello".into()));
    }
    let file = dir.join(format!("{id}.json.result"));
    match std::fs::read(file) {
        Ok(bytes) => Ok(
            serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string())?["status"]
                .as_str()
                .map(str::to_owned),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
