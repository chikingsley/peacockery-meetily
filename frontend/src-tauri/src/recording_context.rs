//! Capture intent and calendar provenance, saved with each recording.
use peacockery_voice::LiveMode;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingMode {
    InPerson,
    Online,
    Phone,
}

impl RecordingMode {
    pub fn captures_system(self) -> bool {
        self != Self::InPerson
    }
    pub fn live_mode(self) -> LiveMode {
        if self == Self::InPerson {
            LiveMode::Mixed
        } else {
            LiveMode::Split
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalendarPerson {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub location: String,
    pub online: bool,
    pub organizer: Option<CalendarPerson>,
    pub invitees: Vec<CalendarPerson>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSnapshot {
    pub observed: String,
    pub events: Vec<CalendarEvent>,
    #[serde(default)]
    pub calls: Vec<CallTask>,
    #[serde(default)]
    pub calls_stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallTask {
    #[serde(default)]
    pub editable: bool,
    pub id: String,
    pub title: String,
    pub number: String,
    pub due: String,
    #[serde(default, rename = "highPriority")]
    pub high_priority: bool,
    pub company: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingContext {
    pub mode: RecordingMode,
    pub calendar_event: Option<CalendarEvent>,
    #[serde(default)]
    pub brief_key: Option<String>,
}

impl RecordingContext {
    pub fn add_event_names(&self, terms: &mut Vec<String>) {
        let Some(event) = &self.calendar_event else {
            return;
        };
        for person in event.organizer.iter().chain(&event.invitees).take(12) {
            let name = person.name.trim();
            if !name.is_empty()
                && !name.contains('@')
                && name.chars().count() <= 50
                && !terms.iter().any(|term| term.eq_ignore_ascii_case(name))
            {
                terms.push(name.to_owned());
            }
        }
    }
    pub fn save(&self, folder: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(folder.join("recording-context.json"), bytes).map_err(|e| e.to_string())
    }
    pub fn load(folder: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(folder.join("recording-context.json")).ok()?).ok()
    }
}

/// Today's meetings from the connected Outlook account; `None` without a calendar.
#[tauri::command]
pub async fn get_recording_calendar(
    app: tauri::AppHandle,
) -> Result<Option<CalendarSnapshot>, String> {
    crate::outlook_calendar::snapshot(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn room_uses_only_mic_and_diarized_mix_calls_capture_both() {
        assert!(!RecordingMode::InPerson.captures_system());
        assert_eq!(RecordingMode::InPerson.live_mode(), LiveMode::Mixed);
        for mode in [RecordingMode::Online, RecordingMode::Phone] {
            assert!(mode.captures_system());
            assert_eq!(mode.live_mode(), LiveMode::Split);
        }
    }
    #[test]
    fn context_survives_later_settings_changes() {
        let dir = tempfile::tempdir().unwrap();
        let context = RecordingContext {
            mode: RecordingMode::InPerson,
            calendar_event: None,
            brief_key: None,
        };
        context.save(dir.path()).unwrap();
        assert_eq!(
            RecordingContext::load(dir.path()).unwrap().mode,
            RecordingMode::InPerson
        );
    }
}
