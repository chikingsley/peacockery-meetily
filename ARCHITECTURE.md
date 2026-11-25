# Peacockery Meetily – architecture and contract

Internal Windows meeting and call recorder for the CICA PC. It is a trimmed fork of Meetily (Tauri 2 + Rust + Next.js). It runs **no local models**: live and final transcription and summaries all go through the Peacockery Voice API (`https://voice-lab.peacockery.studio`).

This file is the contract between the Rust core, the `peacockery-voice` client crate, the `app-loopback` crate and the Next.js frontend. Change it when the contract changes.

## 1. Layout

```text
Cargo.toml                      workspace: frontend/src-tauri, crates/peacockery-voice, crates/app-loopback
crates/peacockery-voice/        Peacockery Voice API client (HTTP + realtime WebSocket), pure logic + tests
crates/app-loopback/            Windows process-tree loopback capture (Chrome / Google Voice audio)
frontend/                       Next.js 16 static export (UI)
frontend/src-tauri/             Tauri app: audio capture, recording, SQLite, commands, events
scripts/build-windows.sh        cross-compile the NSIS installer on GMK (cargo-xwin)
scripts/install-cica.sh         copy + silent install on CICA over ssh
```

The fork removed local Whisper/Parakeet/llama engines, Silero VAD + ONNX Runtime, Ollama and the other LLM providers, PostHog analytics, legacy Python backend, legacy DB import, macOS capture and GPU feature flags. Distribution uses signed Tauri updates, per-installation activation codes and the release workflow documented in `DISTRIBUTION.md`.

Windows is the only shipped target. The workspace also compiles on Linux so `cargo test` runs on GMK. The non-Windows code is limited to that: cpal device enumeration and lookup in `audio/devices/`, `xdg-open` for folders, the Linux keyring backend, and stubs that return errors for process loopback, Outlook sign-in, DPAPI and opening the website.

Identity: productName `Peacockery Meetily`, identifier `studio.peacockery.meetily`, Cargo package `peacockery-meetily`. The version is synchronized in the frontend package, Tauri configuration and Cargo package. App data: `%APPDATA%\studio.peacockery.meetily\`. Recordings: `%USERPROFILE%\Music\Peacockery Meetily\<title>_<YYYY-MM-DD_HH-MM>\`.

## 2. Credential and settings

`activate_access(code)` redeems an eight-digit invitation at the activation Worker. A Durable Object atomically binds the code to the persistent installation UUID, permits that installation's retries, and enforces seven-day expiry. The Rust command validates the returned host and credential, verifies Voice model access, then saves the credential and settings with rollback. The webview receives the recipient name. Operators use `node scripts/access.mjs invite Name` to resolve a verified contact, ensure a separate Voice user, issue scoped access and return the installer/code. Repeated invites reuse access; `--new-device` creates another revocable credential, and `status`/`revoke` manage it. `/admin/status` returns invitation state and expiry to the operator while preserving the claim. Recipients enter their code.

- API key: Windows Credential Manager, generic credential, target name **`studio.peacockery.meetily/peacockery-voice-api-key`** (via the `keyring` crate, `Entry::new_with_target(TARGET, "studio.peacockery.meetily", "peacockery-voice-api-key")`). The key never reaches the webview or SQLite. Only `activate_access` writes it, from the app running in the desktop session (an ssh session cannot write it).
- Settings: Rust-owned store file `voice-settings.json` (tauri-plugin-store), shape `VoiceSettings`:

```jsonc
{
  "base_url": "https://voice-lab.peacockery.studio",
  "live_enabled": true,
  "live_model": "elevenlabs-scribe_v3",
  "final_model": "elevenlabs-scribe_v3",
  "final_diarize": true,
  "auto_final": "when_live_incomplete", // or "always" / "never": the final transcript after stop (§6)
  "language": "auto",              // ISO-639-1, or "auto"
  "keyterms": [],                // vocabulary hints (names, companies); sent where the model supports them
  "transcript_corrections": [],  // [{heard, replacement}]: whole-word replacements in final transcript text
  "me_label": "Me",
  "them_label": "Them",
  "summary_model": "google-gemini-3.8-flash",
  "summary_fallback_models": ["deepseek-deepseek-v4-flash", "google-gemini-3.1-flash-lite"],
  "auto_summary": false
}
```

Every field is required when the frontend saves. The stored file is read one key at a time: an unknown key is logged and ignored, and an invalid value keeps its default; the next save writes the current shape. The live topology and diarization are not settings: the recording mode chooses the topology (§8) and the live model decides diarization (§5).

Model allow-lists (labels shown in the UI; availability = present in `GET /v1/models`):

| Use     | id                                           | Label and note                                                                        |
| ------- | -------------------------------------------- | ------------------------------------------------------------------------------------- |
| live    | `elevenlabs-scribe_v3`                       | Scribe v3 Live (via Superwhisper) – default; live speaker labels                      |
| live    | `deepgram-nova-3`                            | Deepgram Nova (via Superwhisper) – automatic fallback; lowest latency, speaker labels |
| live    | `mistral-voxtral-mini-transcribe-realtime-3` | Voxtral Mini Realtime 3 – more accurate, live speaker labels                          |
| live    | `cohere-transcribe`                          | Cohere Transcribe (self-hosted on GMK) – free, may be offline                         |
| final   | `elevenlabs-scribe_v3`                       | Scribe v3 – default; diarization and word timestamps                                  |
| final   | `mistral-voxtral-small-transcribe-3`         | Voxtral Small Transcribe 3 – top overall WER; segment timestamps                      |
| final   | `mistral-voxtral-mini-transcribe-3`          | Voxtral Mini Transcribe 3                                                             |
| final   | `deepgram-nova-3`                            | Deepgram Nova (via Superwhisper)                                                      |
| final   | `cohere-transcribe`                          | Cohere Transcribe (GMK) – free, no diarization, may be offline                        |
| summary | `google-gemini-3.8-flash`                    | Gemini 3.8 Flash – default, ~5 s on a 1-hour call                                     |
| summary | `anthropic-claude-sonnet-5-5`                | Claude Sonnet 5.5 – best notes; near the API's 10 s cap on long calls                 |
| summary | `deepseek-deepseek-v4-flash`                 | DeepSeek V4 Flash                                                                     |
| summary | `google-gemini-3.1-flash-lite`               | Gemini 3.1 Flash Lite – fastest                                                       |
| summary | `openai-gpt-5.4-mini`                        | GPT-5.4 mini                                                                          |

## 3. Peacockery Voice API facts (verified live 2026-11-09)

- Auth `Authorization: Bearer <pv_lab_…>`; lab keys only work on the lab host. Optional headers: `X-Peacockery-App-ID: peacockery-meetily`, `X-Peacockery-App-Version`, `X-Peacockery-Platform: windows`, `X-Peacockery-Installation-ID` (uuid persisted in the settings store), `X-Peacockery-Client-Session-ID` (meeting id).
- Realtime: `wss://<host>/v1/realtime?model=<routes.realtime.model>&encoding=linear16&sample_rate=16000&language=en[&diarize=true][&target_streaming_delay_ms=480][&keyterm=..][&context_bias=..]`. Binary frames = mono s16le PCM, send 100 ms (3200 bytes) frames continuously (silence too; never gate). Close with text `{"type":"close"}` and wait for `session.completed` / `session.failed` (≤ 20 s). Unknown query params → HTTP 400. `context_bias`/`target_streaming_delay_ms`/`turn_detection` are Voxtral-3-only. `keyterm` is for Deepgram/Scribe routes.
- Observed event behaviour per model (fixtures in `crates/peacockery-voice/tests/fixtures/`):
  - Voxtral realtime 3, no diarize: only `transcript.delta` word fragments (append verbatim), no committed events.
  - Voxtral realtime 3, `diarize=true`: only `transcript.committed` fragments, each `segments[0].speaker` = `speaker_N`, times null.
  - Nova-3 (lab): `transcript.interim` (full current hypothesis) + `transcript.committed` lines with `segments[].startSeconds/endSeconds`.
  - Every session: `session.started {session_id}` first; terminal `session.completed {result}` or `session.failed {error}`.
- Batch: `POST /v1/uploads {content_type, filename, size_bytes}` → `transfer.kind` `single` (PUT url+headers) or `multipart` (parts with url, keep ETag) → `POST /v1/uploads/{id}/complete {parts:[...]}` → `POST /v1/transcriptions {asr_model, input_key, diarize, language?, provider_options?}` → poll `GET /v1/jobs/{id}` (2 s, backing off to 10 s) until `succeeded|failed`. A 150 s clip finishes in ~8–10 s.
  - Scribe batch (fixture `b-scribe.json`, recorded with Scribe v2, since removed): `result.content.words.items[{start_seconds,end_seconds,text,speaker:"speaker_N"}]`; `segments`/`speaker_turns` empty (`provider_omitted`) → group words into turns client-side.
  - Voxtral 3 batch: `result.content.speaker_turns.items[{start_seconds,end_seconds,text,speaker}]` (and segments).
  - Keyterms: `provider_options.elevenlabs.keyterms` (Scribe), `provider_options.mistral.contextBias` (Voxtral), `provider_options.deepgram.keyterm` (Nova).
- Text: `POST /v1/text {model, messages, maxOutputTokens, context:{conversationId,turnId}, routing:{fallbackModels}}` → `{text, model, provider, usage, finishReason}`. Hard 10 s per attempt server-side.

## 4. Audio pipeline

- Capture (cpal WASAPI): mic = selected capture device; system = selected render endpoint loopback, or the pseudo-device **`app:chrome`** = process-tree loopback of the Chrome browser process (Google Voice is a Chrome PWA on CICA) via `crates/app-loopback`. Each source → mono → 48 kHz → `AudioChunk{device_type}`.
- `AudioPipeline::run` aligns mic and system into equal windows (mic is the master clock; system is zero-filled only while its stream is idle; system drift beyond 1.2 s is trimmed).
- From each aligned window (`mic_window`, `sys_window`, `mixed`):
  1. recording path: the mix → raw PCM journal `.checkpoints/audio.s16le`, synced to disk every 2 s with no process started → closed at stop → encoded to `audio.mp4` (96 kbps mono) in the background after stop returns, about 25 s per recorded hour;
  1. live path: resample 48 kHz → 16 kHz, f32 → i16, push to the live sessions (`split`, online and phone: mic → one session, system → another; `mixed`, in person: the mix → one session);
  1. levels: `audio-levels` event at ~10 Hz.
- Pause keeps live sessions alive by sending silence frames that do not advance the recording clock.
- During a recording, `audio/device_monitor.rs` polls for the microphone every 2 s. Discovery runs on a blocking worker shared with Settings. Stopping the monitor cancels its wait independently of the driver. Missing for 4 s (6 s for a Bluetooth-looking name), it triggers the fallback to the system default input, at most three failed attempts; the fallback is kept if the old device returns. System sources are not monitored.
- Recording time 0 is the moment the pipeline starts, after both streams are open (an endpoint loopback takes about 3 s to open on CICA; audio before that is not recorded). The recording clock (`duration_seconds`) and the live line wall-clock timestamps count from there.
- The pipeline returns `PipelineStats` (per-source chunks, samples, peak, RMS; aligned, dropped and zero-filled samples) in `StoppedRecording.stats`.
- Background audio save (`audio/audio_save.rs`): stop registers the meeting as saving before it reads as stopped, then one worker encodes the queued journals one at a time (three attempts each). Every ffmpeg the app starts runs at below-normal priority inside a Windows job object that ends it with the app, so a new recording keeps priority and no encode outlives the app. The final transcript, publication, quit and updates wait for pending saves; quit waits at most five minutes, and an unfinished journal is encoded at the next start.
- `frontend/src-tauri/examples/capture_smoke.rs` runs capture → pipeline → journal → split live sessions → the background encode into `audio.mp4` → final transcript headless with the app's own code and writes a JSON report with `stop_seconds` and `encode_seconds` (`capture_smoke <report.json> [seconds] [default|app:chrome]`, from the desktop session, ffmpeg on `PATH`).

## 5. Live transcript (crate `peacockery_voice::live`)

`LineBuilder` turns realtime events into display lines, per session (channel):

- interim-style models (Nova, Scribe, Cohere): `interim` → preview text; `committed` → final line (times from segments when present, else from the recording clock).
- fragment-style models (Voxtral): fragments (delta or diarized committed) are appended to the open line; a line is finalized when (a) the speaker changes, (b) it ends with `.?!` and is ≥ 6 s or ≥ 18 words, (c) no fragment arrived for 1.5 s, (d) it reaches 30 s, or (e) the session ends.
- Times: the builder is told the current recording time of audio sent; a fragment's audio time ≈ sent_time − streaming_delay (0.5 s default). Line start = first fragment time, end = last fragment time.
- Diarization is requested for Scribe v3, Voxtral realtime 3 and the Nova-3 fallback (`diarize_for_model`), on every session.
- Speaker for a line: split microphone with diarization → `Local speaker 1/2/…`; split system with diarization → `them_label 1/2/…`; mixed with diarization → `Speaker 1/2/…`. A model without live diarization (Cohere) labels split lines `me_label` / `them_label` and leaves mixed lines unlabelled. A shared microphone never establishes a single person.

### Scribe v3 live delivery

Scribe v3 waits for the diarized commit before saving each passage. The API preserves word times and speaker turns, and the app splits multi-speaker commits into separate lines. On a provider rejection or two transient failures, each Scribe channel switches to Nova while local audio capture continues; the live transcript is then incomplete and the final transcript runs (§6). Nova is diarized too, so its lines keep numbered labels. Authentication failures remain actionable errors. The API allows eight seconds for delayed labels after stop. The capture tap and reconnect queue each hold up to 60 seconds per feed. A socket write times out after five seconds. A failed replay keeps the frame for the next connection.

## 6. Final transcript

Rust owns it (`final_transcript.rs`), keyed by meeting id. Scribe v3 is the final model by default.

When it runs automatically after a stop or a crash recovery (`auto_final`): `always`; `never` (run it from the meeting page); `when_live_incomplete` (default) runs it unless the live transcript completed on the final model (`live_complete = 1` and `live_model = final_model`), in which case the live transcript is the result. A channel that falls back to Nova, loses a session (audio the provider had not committed), drops buffered audio while connecting, exhausts its 60-second tap buffer, gives up, stops while failing or reconnecting, ends with `session.failed` at stop, or does not finish cleanly, a live line that cannot be stored, and the 20 s stop timeout, each mark the live transcript incomplete. Recovered meetings and recordings without live sessions have `live_complete` NULL, so they run. A needed run is queued (`queued` stored) after live sessions drain in the background. Imports always run it. When no final transcript runs, `auto_summary` summarizes the live rows. Retry and startup resume use the run's stored model while it is still in the final list, else `final_model`.

1. A run first waits for the meeting's background audio save (state `queued`, message "Saving audio…"); a failed encode fails the run with its error.
1. Sources over 20 MiB are temporarily encoded as one complete mono 16 kHz AAC file at 32 kbit/s, with the original file and timeline preserved. The temporary file is removed on completion, failure or cancellation. `audio.mp4` (or its complete encoded copy) → upload → job (`final_model`, `final_diarize`, `language`, keyterms) → poll.
1. Turns: use `speaker_turns` when non-empty, else group `words` (same speaker, gap < 1.5 s, split at sentence end after 30 s), else `segments`, else one segment with the full text.
1. Speaker naming: provider speakers become `Speaker 1`, `Speaker 2`… in order of first appearance; turns without a provider speaker stay unnamed. Calendar invitees do not identify voices.
1. Replace rows `transcripts WHERE meeting_id=? AND source='final'`; keep `source='live'` rows. Save `final-transcript.json` (turns) and `final-artifact.json` (raw API result) in the meeting folder.
1. Update meeting `final_status/final_model/final_job_id/final_error`; emit `final-transcript-status`.
1. If `auto_summary`, start the summary afterwards.

Automatic summaries use the saved live transcript when no final transcript runs. The meeting page receives summary status events as background finalization progresses. A final transcript takes precedence when it runs; it is queued at the same point and waits for the background audio encode.

## 7. Database (SQLite via sqlx; migrations `20261109000000_initial.sql`, `20261117000000_publications.sql`, `20261118000000_live_complete.sql`)

The CICA database holds real meetings, so schema changes are new additive migrations; existing migrations are never edited.

```sql
meetings(id TEXT PK, title TEXT NOT NULL, created_at TEXT, updated_at TEXT, folder_path TEXT,
         status TEXT NOT NULL DEFAULT 'recording',      -- recording | stopped | interrupted | imported
         duration_seconds REAL, live_model TEXT, live_mode TEXT, realtime_session_ids TEXT,  -- JSON array
         final_status TEXT, final_model TEXT, final_job_id TEXT, final_error TEXT, final_updated_at TEXT,
         live_complete INTEGER)                         -- 1/0 set at stop; NULL = unknown (§6)
transcripts(id TEXT PK, meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
            source TEXT NOT NULL,                       -- live | final
            speaker TEXT, transcript TEXT NOT NULL, timestamp TEXT,
            audio_start_time REAL, audio_end_time REAL, duration REAL, sequence INTEGER)
            INDEX (meeting_id, source, audio_start_time)
summary_processes(meeting_id TEXT PK REFERENCES meetings(id) ON DELETE CASCADE, status TEXT, created_at TEXT,
                  updated_at TEXT, error TEXT, result TEXT, start_time TEXT, end_time TEXT,
                  processing_time REAL, metadata TEXT, result_backup TEXT, result_backup_timestamp TEXT)
meeting_publications(meeting_id TEXT PK REFERENCES meetings(id) ON DELETE CASCADE,
                     state TEXT NOT NULL DEFAULT 'queued', upload_id TEXT, parts_json TEXT NOT NULL DEFAULT '[]',
                     bytes_sent, total_bytes, attempts, retry_at INTEGER NOT NULL DEFAULT 0, error TEXT)
                     -- one row per meeting queued for CICA publication (Automatic publication below)
```

SQLite maintains the review transcript count on insert, delete and move. An index on meeting ID supports the review worker’s last-row lookup.

"Best" transcript of a meeting = `final` rows if any exist, else `live` rows. On startup, meetings still `recording` become `interrupted`, and every meeting whose folder still holds an audio journal (interrupted, failed encode, or quit before the background encode finished) is registered as saving before unfinished final transcripts resume, then encoded on the background worker. Unless `auto_final` is `never`, the final transcript runs for every interrupted meeting once it has an `audio.*` file (including one whose `audio.mp4` was already written before the crash, with no journal left); a meeting that was already stopped gets its audio encoded: its queued final transcript resumes and waits for it, and a final the §6 rule needs that never ran or failed waiting for the audio starts once the audio is saved. Its automatic summary is not repeated.

## 8. Tauri commands (frontend → Rust)

Recording:

- `recording_mode` is `in_person`, `online` or `phone`; setup preselects in person, and a selected calendar event suggests a mode. In person requires a microphone, captures no system source and uses one mixed session. Online and phone capture both selected sources with split sessions. Sidebar and tray actions open setup; capture begins only at the recording screen Start button.
- `calendar_event` optionally carries the selected occurrence ID, title, start/end, location, online flag, organizer and invitees. It is stored with `mode` in `recording-context.json` beside the audio before capture starts.
- `get_recording_calendar() -> CalendarSnapshot|null` reads the selected calendar of the connected account through the Outlook REST calendar route, with pagination and UTC times. With no selected calendar or no saved session it returns null without contacting Microsoft. `outlook_connect`, `outlook_status`, `outlook_select` and `outlook_disconnect` manage a dedicated Edge session. `outlook-session.bin` uses Windows DPAPI with app-specific entropy; expired tokens trigger headless session capture, with a five-minute backoff after failure. Background browser throttling is disabled so sign-in progresses while another app has focus. Disconnect clears the selected calendar (`outlook-calendar.json` → `{"calendar":""}`), deletes the session and removes the browser profile. The UI shows Arizona-today events. Recording remains available independently.
- `get_audio_devices() -> AudioDevice[]` – includes the pseudo system device `{name:"app:chrome", device_type:"Output", label:"Google Chrome app audio (Google Voice)"}` on Windows. Only pseudo devices carry a `label`; the UI shows it, else the name.
- `start_recording_with_devices_and_meeting(mic_device_name?, system_device_name?, meeting_name, recording_mode, calendar_event?, publish_to_cica) -> {meeting_id, folder_path}` – a null device is the system default; an empty `meeting_name` is rejected. Setup seeds the devices from the saved preferences and sends `Meeting <YYYY-MM-DD HH:MM>` when the title is blank; a summary may rename that placeholder title.
- `stop_recording() -> {meeting_id, folder_path}` – resolves after capture drains, the audio journal closes and the meeting is marked stopped. Live-session shutdown, transcript writes and automatic processing continue independently for that meeting; `audio.mp4` is encoded afterwards and reported by `recording-audio`. A journal that cannot be closed (no audio, disk error) is reported by one `recording-error` event (the command does not also reject)
- `pause_recording()`, `resume_recording()`, `get_recording_state() -> {is_recording, is_stopping, is_paused, is_active, meeting_id|null, …durations, local_progress?}` (`local_progress = {received, saved, write_failed, mic_gap_seconds}`: seconds of audio received from capture and synced to the journal, present while the recording runs. Microphone hardware timestamp gaps of at least 250 ms accumulate in recording health. Pause, resume and device changes start a new measurement sequence), `get_transcript_history() -> LiveLine[]` (current session)
- `get_recording_preferences()`, `set_recording_preferences(prefs)`, `get_default_recordings_folder_path()`, `open_recordings_folder()` (no `auto_save` toggle – audio is always saved)
- `start_audio_level_monitoring(device_names)`, `stop_audio_level_monitoring()` – real RMS (settings device test)

Voice settings:

- `voice_get_settings() -> VoiceSettings`, `voice_save_settings(settings) -> VoiceSettings` (the full shape; emits `voice-settings-updated`)
- `activate_access(code) -> string` – the recipient name (§2); the UI then re-reads `voice_get_settings`
- `voice_get_key_status() -> KeyStatus = {configured: bool, masked: string|null}`
- `voice_test_connection() -> {ok: bool, environment: string|null, latency_ms: number, error: string|null}`
- `voice_list_models() -> {live: ModelOption[], final: ModelOption[], summary: ModelOption[], error: string|null}` (`error` set when `GET /v1/models` failed; the curated lists are still returned, all marked unavailable) where `ModelOption = {id, label, note, available: bool, supports_diarization: bool, recommended: bool}`

Meetings / transcripts:

- `api_get_meetings() -> {id, title, created_at, final_status}[]`, `api_search_transcripts(query)`
- `api_get_meeting_metadata(meeting_id) -> MeetingMetadata` (all meeting columns, `live_complete: bool|null` included, + `audio_path: string|null` – the folder's `audio.*` file – + `audio_saving: bool` – the audio is still being encoded from the journal – + `has_live`, `has_final`, optional `recording_context`)
- `api_get_meeting_transcripts(meeting_id, limit, offset, source?: "best"|"live"|"final") ->  {transcripts: Transcript[], total_count, has_more, source: "live"|"final"}` where `Transcript = {id, text, timestamp, speaker|null, source, audio_start_time, audio_end_time, duration}`
- `api_save_meeting_title(meeting_id, title)`, `api_delete_meeting(meeting_id)`, `open_meeting_folder(meeting_id)`
- `rename_speaker(meeting_id, from, to) -> number` – renames on `final` rows (and `live` rows) of that meeting; returns rows changed
- `start_final_transcript(meeting_id, model?: string)` – (re)run the final transcript; returns immediately
- `get_final_transcript_status(meeting_id) -> {state, model, error}`
- Import: `select_and_validate_audio_command()`, `validate_audio_file_command(path)`, `start_import_audio_command(source_path, title?) -> {meeting_id}` (copies the file into a new meeting folder, then runs the final transcript), `cancel_import_command()`

Summaries:

- `api_process_transcript(meeting_id, template_id?, custom_prompt?, model?) -> {process_id}`
- `api_get_summary(meeting_id)`, `api_save_meeting_summary(meeting_id, summary)`, `api_cancel_summary(meeting_id, process_id)`, `api_list_templates()`

## 9. Events (Rust → frontend)

- `recording-starting`, `recording-started {meeting_id, folder_path}`, `recording-paused`, `recording-resumed`
- `recording-stopping {meeting_id}` – a stop began (button, tray or quit); the UI shows "Stopping recording…" until `recording-stopped` or `recording-error`
- `recording-stopped {meeting_id, folder_path, meeting_name}`
- `recording-audio {meeting_id, state:"saved"|"failed", message|null}` – the background encode into `audio.mp4` finished (after a stop or startup recovery); a failure also emits `recording-error` saying the recording is safe on disk and will be encoded at the next start. The meeting page re-reads its metadata.
- `open-recording-setup` – the tray opens recording setup; capture starts at its Start button
- `transcript-update` – one finalized live line: `{meeting_id, sequence_id, text, speaker|null, channel:"mic"|"system"|"mixed", timestamp:"HH:MM:SS" (local),   audio_start_time, audio_end_time, duration, is_partial:false, source:"live", confidence:null}` (`sequence_id` restarts at 0 per recording and is unique within it)
- `transcript-interim {meeting_id, channel, speaker|null, text}` – replaces that channel's preview line; `text:""` clears it
- `live-transcription-status {meeting_id, channel, model, state:"connecting"|"live"|"reconnecting"|"failed"|"closed", message|null}`
- `audio-levels {mic: number, system: number, system_silent_seconds: number}` – levels 0..1 (dBFS mapped −60..0); ~10 Hz
- `final-transcript-status {meeting_id, state:"queued"|"uploading"|"transcribing"|"done"|"failed", model, message|null, segments_count|null}`
- `mic-device-switched`, `mic-unavailable`, `mic-swap-failed`, `mic-recovery-exhausted`, `recording-error {message}`
- `import-progress`, `import-complete {meeting_id}`, `import-error`
- `voice-settings-updated` – the saved `VoiceSettings`
- Summaries emit no events: the meeting page polls `api_get_summary` every 5 s.

## Automatic publication

`publication.rs` owns a persistent `meeting_publications` queue. `publication-settings.json` stores the default for future recordings (true when absent). `publish_to_cica` is a required argument of the start command; recording setup shows the choice, preset from that default. A true choice creates a queue row with the new meeting. Meetings recorded before the queue existed and imports have no row. The worker waits for the stopped/interrupted meeting state and skips meetings whose audio is still saving (or, for meetings this run has not registered, still in a journal) without spending an attempt, then sends the original audio through the existing CICA multipart upload API. The website generates its own notes and transcript; local edits retain their local scope.

Commands: `publication_default`, `publication_set_default(enabled)`, `publication_status(meeting_id)`, `publication_retry(meeting_id)` and `publication_open(meeting_id)`. States: queued, uploading, processing, published, failed. Status includes bytes sent, total bytes, error and website URL. The database persists upload ID and part receipts; retry first reconciles the remote ID to avoid duplicate publication after a lost completion response. The UI polls every three seconds; the worker checks every twenty seconds with a one-minute failure backoff and five-attempt limit for uploads. Processing state continues polling the website. Data preservation covers the original recording, local transcripts, settings and all historical meetings.

## 10. Build and install

- GMK: `scripts/build-windows.sh` → `pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis --no-sign`. ffmpeg is downloaded at build time and verified by pinned SHA-256 (never executed during the build).
- CICA: `scripts/install-cica.sh` → `scp` the setup exe, silent per-user install (`/S`). The key cannot be stored over ssh (Credential Manager refuses a network logon session): enter the activation code in the app in the desktop session.
- Logs: `%LOCALAPPDATA%\studio.peacockery.meetily\logs\` (tauri-plugin-log), readable over `ssh cica`.

## 11. Frontend

Next.js 16, React 19, Tailwind 4 and BlockNote 0.57 build the static export that Tauri serves. Turbopack builds the app with React Strict Mode enabled. Tiptap and ProseMirror come only through BlockNote; the app pins neither. Radix primitives come from the unified `radix-ui` package, and class names merge through the `cn` package's `cn` helper (`components.json` `utils: "cn"`).

The shadcn shell (`components/ui/sidebar.tsx`) owns the sidebar collapse state, kept in localStorage. Meeting data lives in `contexts/MeetingContext.tsx` (`MeetingProvider`, `useMeetings`); `hooks/meeting-details/useSummaryPolling.ts` owns summary polling. New meeting and the tray's `open-recording-setup` event open recording setup; Start recording sends the title, mode, calendar event, publication choice and selected devices to Rust.

Search debounces 200 ms and invalidates prior responses immediately, including on clear and unmount. Rust returns at most 100 distinct meeting hits, newest first, from the best transcript. SQL wildcard characters are escaped so search terms are literal. Indexed full-text search remains a separate scaling option.

Ultracite's core, React, Next.js, anti-slop, and shadcn presets run through Oxlint. Oxfmt uses Ultracite's formatter preset through explicit config. Project exceptions are documented inline: component naming, native callback APIs, compiler-only checks, runtime parsers, native test fixtures, and measured transcript virtualization. `pnpm check` runs the complete frontend gate.

### Task call attempts

A task has one Trello checklist item and can have several dial destinations. The CRM supplies the primary number. An alternate can be written as “, call Jake Zemer at 480-989-3538” or “, then the office at 480-967-0080”. Each recording start accepts an optional contact with a name and number. For a task origin, the recorder writes one SQLite `call_destinations` row keyed by meeting ID, including attempts started without a number. A failed start rolls the attempt back with the meeting. The existing recording context preserves the task key for stop navigation from the button, tray or shortcut.

`call_attempts` returns recent task recordings, their destinations, automatic review results and any manual correction. `set_call_outcome` sets or clears the correction independently of the review worker. Ordinary recordings still open their meeting details. Recording, reviewing and correcting a call leave Trello completion and due dates under the control of the existing task workflow.

### Calendar progress and call task edits

The agenda uses Phoenix time and the Outlook occurrence ID saved in the recording context. Ended appointments open their linked recording. The live view shows the remaining scheduled time. Call tasks linked to a Trello checklist can be completed or rescheduled through `change_call_task`. The app atomically queues one edit per CRM task in its private `call-changes` folder. The authenticated GMK agenda publisher rechecks the assigned task, checklist wording, prior due date and state, writes the Trello item, verifies its response, then refreshes the agenda. `call_change_status` reads the receipt. Recording a voicemail updates the attempt and leaves the business objective open.

On Windows, Settings → Recording controls meeting focus on this PC. At recording start, a serialized background worker snapshots the current session's ChatGPT/Codex, Claude and Gemini desktop process trees by installed path and process creation time. It saves the apps to reopen, requests window closure, terminates survivors and checks for exit twice. On stop, exit, update or startup recovery it reopens each saved app using its Start menu identity. Recording runs independently. Reopening an app restores its desktop entry point. Jobs that were running inside it may have ended.

Scheduled captures append up to twelve calendar display names to saved vocabulary hints for live and final ASR. The recorded context supplies the same names on a later final pass. Names guide spelling; speaker identification requires its own evidence.
