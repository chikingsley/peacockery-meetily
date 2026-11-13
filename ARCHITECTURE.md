# Peacockery Meetily — architecture and contract

Internal Windows meeting and call recorder for the CICA PC. It is a trimmed fork of Meetily
(Tauri 2 + Rust + Next.js). It runs **no local models**: live and final transcription and
summaries all go through the Peacockery Voice API (`https://voice-lab.peacockery.studio`).

This file is the contract between the Rust core, the `peacockery-voice` client crate, the
`app-loopback` crate and the Next.js frontend. Change it when the contract changes.

## 1. Layout

```
Cargo.toml                      workspace: frontend/src-tauri, crates/peacockery-voice, crates/app-loopback
crates/peacockery-voice/        Peacockery Voice API client (HTTP + realtime WebSocket), pure logic + tests
crates/app-loopback/            Windows process-tree loopback capture (Chrome / Google Voice audio)
frontend/                       Next.js 16 static export (UI)
frontend/src-tauri/             Tauri app: audio capture, recording, SQLite, commands, events
scripts/build-windows.sh        cross-compile the NSIS installer on GMK (cargo-xwin)
scripts/install-cica.sh         copy + silent install on CICA over ssh
```

Removed for good: local Whisper/Parakeet/llama engines, Silero VAD + ONNX Runtime, Ollama and
the other LLM providers, PostHog analytics, the updater, onboarding, legacy Python backend,
legacy DB import, macOS capture, GPU feature flags, signing, release workflows.

Windows is the only shipped target. The crate also compiles on Linux so `cargo test` runs on GMK;
the only Linux-specific code is cpal device enumeration and lookup in `audio/devices/` (no process loopback).

Identity: productName `Peacockery Meetily`, identifier `studio.peacockery.meetily`, Cargo package
`peacockery-meetily`, version `0.6.0`. App data: `%APPDATA%\studio.peacockery.meetily\`.
Recordings: `%USERPROFILE%\Music\Peacockery Meetily\<title>_<YYYY-MM-DD_HH-MM>\` (Music is not
OneDrive-redirected on CICA; never fall back to Documents).

## 2. Credential and settings

- API key: Windows Credential Manager, generic credential, target name
  **`studio.peacockery.meetily/peacockery-voice-api-key`** (via the `keyring` crate,
  `Entry::new_with_target(TARGET, "studio.peacockery.meetily", "peacockery-voice-api-key")`).
  The key never reaches the webview or SQLite. Paste it in Settings, or run `cmdkey /generic:<target>
  /user:peacockery-voice-api-key /pass:<key>` in the desktop session (an ssh session cannot write it).
- Settings: Rust-owned store file `voice-settings.json` (tauri-plugin-store), shape `VoiceSettings`:

```jsonc
{
  "base_url": "https://voice-lab.peacockery.studio",
  "live_enabled": true,
  "live_model": "elevenlabs-scribe_v3",
  "live_mode": "split",          // "split": mic -> "Me" session + system -> "Them" session; "mixed": one session on the mix
  "live_diarize": true,          // Scribe v3 and Voxtral realtime: speaker labels on the mixed stream (mixed) or system stream (split)
  "final_model": "elevenlabs-scribe_v3",
  "final_diarize": true,
  "auto_final": false,           // preserve the live transcript; final batch is optional
  "language": "en",              // ISO-639-1, or "auto"
  "keyterms": [],                // vocabulary hints (names, companies); sent where the model supports them
  "me_label": "Me",
  "them_label": "Them",
  "summary_model": "google-gemini-3.8-flash",
  "summary_fallback_models": ["deepseek-deepseek-v4-flash", "google-gemini-3.1-flash-lite"],
  "auto_summary": false
}
```

Model allow-lists (labels shown in the UI; availability = present in `GET /v1/models`):

| Use | id | Label / note |
|---|---|---|
| live | `elevenlabs-scribe_v3` | Scribe v3 Live (via Superwhisper) — default; live speaker labels |
| live | `deepgram-nova-3` | Deepgram Nova (via Superwhisper) — automatic fallback; lowest latency, channel labels |
| live | `mistral-voxtral-mini-transcribe-realtime-3` | Voxtral Mini Realtime 3 — more accurate, live speaker labels |
| live | `elevenlabs-scribe_v2` | Scribe v2 Realtime (via Superwhisper) — accurate, less reliable |
| live | `cohere-transcribe` | Cohere Transcribe (self-hosted on GMK) — free, may be offline |
| final | `elevenlabs-scribe_v3` | Scribe v3 — optional final pass; diarization and word timestamps |
| final | `elevenlabs-scribe_v2` | Scribe v2 — legacy; diarization + word timestamps |
| final | `mistral-voxtral-small-transcribe-3` | Voxtral Small Transcribe 3 — top overall WER; segment timestamps |
| final | `mistral-voxtral-mini-transcribe-3` | Voxtral Mini Transcribe 3 |
| final | `deepgram-nova-3` | Deepgram Nova (via Superwhisper) |
| final | `cohere-transcribe` | Cohere Transcribe (GMK) — free, no diarization, may be offline |
| summary | `google-gemini-3.8-flash` | Gemini 3.8 Flash — default, ~5 s on a 1-hour call |
| summary | `anthropic-claude-sonnet-5-5` | Claude Sonnet 5.5 — best notes; near the API's 10 s cap on long calls |
| summary | `deepseek-deepseek-v4-flash` | DeepSeek V4 Flash |
| summary | `google-gemini-3.1-flash-lite` | Gemini 3.1 Flash Lite — fastest |
| summary | `openai-gpt-5.4-mini` | GPT-5.4 mini |

## 3. Peacockery Voice API facts (verified live 2026-11-09)

- Auth `Authorization: Bearer <pv_lab_…>`; lab keys only work on the lab host. Optional headers:
  `X-Peacockery-App-ID: peacockery-meetily`, `X-Peacockery-App-Version`, `X-Peacockery-Platform: windows`,
  `X-Peacockery-Installation-ID` (uuid persisted in the settings store), `X-Peacockery-Client-Session-ID` (meeting id).
- Realtime: `wss://<host>/v1/realtime?model=<routes.realtime.model>&encoding=linear16&sample_rate=16000&language=en[&diarize=true][&target_streaming_delay_ms=480][&keyterm=..][&context_bias=..]`.
  Binary frames = mono s16le PCM, send 100 ms (3200 bytes) frames continuously (silence too; never gate).
  Close with text `{"type":"close"}` and wait for `session.completed` / `session.failed` (≤ 20 s).
  Unknown query params → HTTP 400. `context_bias`/`target_streaming_delay_ms`/`turn_detection` are Voxtral-3-only.
  `keyterm` is for Deepgram/Scribe routes.
- Observed event behaviour per model (fixtures in `crates/peacockery-voice/tests/fixtures/`):
  - Voxtral realtime 3, no diarize: only `transcript.delta` word fragments (append verbatim), no committed events.
  - Voxtral realtime 3, `diarize=true`: only `transcript.committed` fragments, each `segments[0].speaker` = `speaker_N`, times null.
  - Nova-3 (lab): `transcript.interim` (full current hypothesis) + `transcript.committed` lines with `segments[].startSeconds/endSeconds`.
  - Scribe v2 realtime (lab): `transcript.interim` + a few long `transcript.committed`.
  - Every session: `session.started {session_id}` first; terminal `session.completed {result}` or `session.failed {error}`.
- Batch: `POST /v1/uploads {content_type, filename, size_bytes}` → `transfer.kind` `single` (PUT url+headers) or
  `multipart` (parts with url, keep ETag) → `POST /v1/uploads/{id}/complete {parts:[...]}` →
  `POST /v1/transcriptions {asr_model, input_key, diarize, language?, provider_options?}` → poll `GET /v1/jobs/{id}`
  (2 s, backing off to 10 s) until `succeeded|failed`. A 150 s clip finishes in ~8–10 s.
  - Scribe v2 (lab): `result.content.words.items[{start_seconds,end_seconds,text,speaker:"speaker_N"}]`;
    `segments`/`speaker_turns` empty (`provider_omitted`) → group words into turns client-side.
  - Voxtral 3 batch: `result.content.speaker_turns.items[{start_seconds,end_seconds,text,speaker}]` (and segments).
  - Keyterms: `provider_options.elevenlabs.keyterms` (Scribe), `provider_options.mistral.contextBias` (Voxtral),
    `provider_options.deepgram.keyterm` (Nova).
- Text: `POST /v1/text {model, messages, maxOutputTokens, context:{conversationId,turnId}, routing:{fallbackModels}}`
  → `{text, model, provider, usage, finishReason}`. Hard 10 s per attempt server-side.

## 4. Audio pipeline

- Capture (cpal WASAPI): mic = selected capture device; system = selected render endpoint loopback, or the
  pseudo-device **`app:chrome`** = process-tree loopback of the Chrome browser process (Google Voice is a
  Chrome PWA on CICA) via `crates/app-loopback`. Each source → mono → 48 kHz → `AudioChunk{device_type}`.
- `AudioPipeline::run` aligns mic and system into equal windows (mic is the master clock; system is
  zero-filled only while its stream is idle; system drift beyond 1.2 s is trimmed).
- From each aligned window (`mic_window`, `sys_window`, `mixed`):
  1. recording path: the mix → incremental AAC checkpoints → `audio.mp4` (96 kbps mono) at stop;
  2. live path: resample 48 kHz → 16 kHz, f32 → i16, push to the live sessions
     (`split`: mic → "Me" session, system → "Them" session; `mixed`: the mix → one session);
  3. energy track: per 100 ms frame RMS dBFS of mic and system → `channel-energy.json` in the meeting folder;
  4. levels: `audio-levels` event at ~10 Hz.
- Pause keeps live sessions alive by sending silence frames that do not advance the recording clock.
- Recording time 0 is the moment the pipeline starts, after both streams are open (an endpoint loopback
  takes about 3 s to open on CICA; audio before that is not recorded). The recording clock
  (`duration_seconds`) and the live line wall-clock timestamps count from there.
- The pipeline returns `PipelineStats` (per-source chunks, samples, peak, RMS; aligned, dropped and
  zero-filled samples) in `StoppedRecording.stats`.
- `frontend/src-tauri/examples/capture_smoke.rs` runs capture → pipeline → `audio.mp4` → live sessions →
  final transcript headless with the app's own code and writes a JSON report
  (`capture_smoke <report.json> [seconds] [default|app:chrome]`, from the desktop session, ffmpeg on `PATH`).

## 5. Live transcript (crate `peacockery_voice::live`)

`LineBuilder` turns realtime events into display lines, per session (channel):
- interim-style models (Nova, Scribe, Cohere): `interim` → preview text; `committed` → final line (times from
  segments when present, else from the recording clock).
- fragment-style models (Voxtral): fragments (delta or diarized committed) are appended to the open line;
  a line is finalized when (a) the speaker changes, (b) it ends with `.?!` and is ≥ 6 s or ≥ 18 words,
  (c) no fragment arrived for 1.5 s, (d) it reaches 30 s, or (e) the session ends.
- Times: the builder is told the current recording time of audio sent; a fragment's audio time ≈
  sent_time − streaming_delay (0.5 s default). Line start = first fragment time, end = last fragment time.
- Speaker for a line: split mode → `me_label` / `them_label` (system session with `live_diarize`:
  `them_label` + ` 1/2/…` from `speaker_N`); mixed + diarize → `Speaker 1/2/…`; otherwise none.

## 6. Final transcript

Rust owns it (`final_transcript.rs`), keyed by meeting id:
1. `audio.mp4` → upload → job (`final_model`, `final_diarize`, `language`, keyterms) → poll.
2. Turns: use `speaker_turns` when non-empty, else group `words` (same speaker, gap < 1.5 s, split at sentence
   end after 30 s), else `segments`, else one segment with the full text.
3. Speaker naming from `channel-energy.json`: for each provider speaker, fraction of its speech time where the
   system channel is active (sys ≥ −50 dBFS and sys ≥ mic − 6 dB). Lowest fraction and < 0.35 → `me_label`.
   Remaining speakers → `them_label` if exactly one, else `them_label 1`, `them_label 2`… in order of first
   appearance. No energy file or no system audio at all → `Speaker 1`, `Speaker 2`….
4. Replace rows `transcripts WHERE meeting_id=? AND source='final'`; keep `source='live'` rows.
   Save `final-transcript.json` (turns) and `final-artifact.json` (raw API result) in the meeting folder.
5. Update meeting `final_status/final_model/final_job_id/final_error`; emit `final-transcript-status`.
6. If `auto_summary`, start the summary afterwards.

## 7. Database (fresh single migration `20261109000000_initial.sql`, SQLite via sqlx)

```sql
meetings(id TEXT PK, title TEXT NOT NULL, created_at TEXT, updated_at TEXT, folder_path TEXT,
         status TEXT NOT NULL DEFAULT 'recording',      -- recording | stopped | interrupted | imported
         duration_seconds REAL, live_model TEXT, live_mode TEXT, realtime_session_ids TEXT,  -- JSON array
         final_status TEXT, final_model TEXT, final_job_id TEXT, final_error TEXT, final_updated_at TEXT)
transcripts(id TEXT PK, meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
            source TEXT NOT NULL,                       -- live | final
            speaker TEXT, transcript TEXT NOT NULL, timestamp TEXT,
            audio_start_time REAL, audio_end_time REAL, duration REAL, sequence INTEGER)
            INDEX (meeting_id, source, audio_start_time)
summary_processes(meeting_id TEXT PK REFERENCES meetings(id) ON DELETE CASCADE, status TEXT, created_at TEXT,
                  updated_at TEXT, error TEXT, result TEXT, start_time TEXT, end_time TEXT,
                  processing_time REAL, metadata TEXT, result_backup TEXT, result_backup_timestamp TEXT)
```

"Best" transcript of a meeting = `final` rows if any exist, else `live` rows.
On startup, meetings still `recording` become `interrupted`; their checkpoints are recovered into `audio.mp4`
and, if `auto_final`, the final transcript runs for every interrupted meeting that has an `audio.*` file
(including one whose `audio.mp4` was already written before the crash, with no checkpoints left).

## 8. Tauri commands (frontend → Rust)

Recording:
- `get_audio_devices() -> AudioDevice[]` — includes the pseudo system device `{name:"app:chrome", device_type:"Output", label:"Google Chrome app audio (Google Voice)"}` on Windows.
- `start_recording_with_devices_and_meeting(mic_device_name?, system_device_name?, meeting_name?) -> {meeting_id, folder_path}`
- `stop_recording() -> {meeting_id, folder_path, audio_path: string|null}` — always resolves once the stop has run;
  a failed save is reported by one `recording-error` event and `audio_path: null` (the command does not also reject)
- `pause_recording()`, `resume_recording()`, `is_recording() -> bool`,
  `get_recording_state() -> {is_recording, is_stopping, is_paused, is_active, meeting_id|null, …durations}`,
  `get_recording_meeting_name()`, `get_transcript_history() -> LiveLine[]` (current session)
- `get_recording_preferences()`, `set_recording_preferences(prefs)`, `get_default_recordings_folder_path()`,
  `open_recordings_folder()` (no `auto_save` toggle — audio is always saved)
- `start_audio_level_monitoring(device_names)`, `stop_audio_level_monitoring()` — real RMS (settings device test)

Voice settings:
- `voice_get_settings() -> VoiceSettings`, `voice_save_settings(settings) -> VoiceSettings` (emits `voice-settings-updated`)
- `voice_get_key_status() -> KeyStatus = {configured: bool, masked: string|null}`, `voice_set_api_key(key) -> KeyStatus`,
  `voice_clear_api_key() -> KeyStatus`
- `voice_test_connection() -> {ok: bool, environment: string|null, latency_ms: number, error: string|null}`
- `voice_list_models() -> {live: ModelOption[], final: ModelOption[], summary: ModelOption[], error: string|null}`
  (`error` set when `GET /v1/models` failed; the curated lists are still returned, all marked unavailable)
  where `ModelOption = {id, label, note, available: bool, supports_diarization: bool, recommended: bool}`

Meetings / transcripts:
- `api_get_meetings() -> {id, title, created_at, final_status}[]`, `api_search_transcripts(query)`
- `api_get_meeting_metadata(meeting_id) -> MeetingMetadata` (all meeting columns + `audio_path: string|null` — the folder's `audio.*` file — + `has_live`, `has_final`)
- `api_get_meeting_transcripts(meeting_id, limit, offset, source?: "best"|"live"|"final") ->
   {transcripts: Transcript[], total_count, has_more, source: "live"|"final"}`
   where `Transcript = {id, text, timestamp, speaker|null, source, audio_start_time, audio_end_time, duration}`
- `api_save_meeting_title(meeting_id, title)`, `api_delete_meeting(meeting_id)`, `open_meeting_folder(meeting_id)`
- `rename_speaker(meeting_id, from, to) -> number` — renames on `final` rows (and `live` rows) of that meeting; returns rows changed
- `start_final_transcript(meeting_id, model?: string)` — (re)run the final transcript; returns immediately
- `get_final_transcript_status(meeting_id) -> {state, model, error}`
- Import: `select_and_validate_audio_command()`, `validate_audio_file_command(path)`,
  `start_import_audio_command(source_path, title?) -> {meeting_id}` (copies the file into a new meeting folder,
  then runs the final transcript), `cancel_import_command()`

Summaries:
- `api_process_transcript(meeting_id, template_id?, custom_prompt?, model?) -> {process_id}`
- `api_get_summary(meeting_id)`, `api_save_meeting_summary(meeting_id, summary)`, `api_cancel_summary(meeting_id, process_id)`, `api_list_templates()`

## 9. Events (Rust → frontend)

- `recording-starting`, `recording-started {meeting_id, folder_path}`, `recording-paused`, `recording-resumed`
- `recording-stopping {meeting_id}` — a stop began (button, tray or quit); the UI shows "Stopping… finishing live transcript"
  until `recording-stopped` or `recording-error`
- `recording-stopped {meeting_id, folder_path, meeting_name}`
- `request-recording-toggle` — the tray asks the frontend to start a recording (the frontend owns device checks and naming)
- `transcript-update` — one finalized live line:
  `{meeting_id, sequence_id, text, speaker|null, channel:"mic"|"system"|"mixed", timestamp:"HH:MM:SS" (local),
    audio_start_time, audio_end_time, duration, is_partial:false, source:"live", confidence:null}`
  (`sequence_id` restarts at 0 per recording and is unique within it)
- `transcript-interim {meeting_id, channel, speaker|null, text}` — replaces that channel's preview line; `text:""` clears it
- `live-transcription-status {meeting_id, channel, model, state:"connecting"|"live"|"reconnecting"|"failed"|"closed", message|null}`
- `audio-levels {mic: number, system: number, system_silent_seconds: number}` — levels 0..1 (dBFS mapped −60..0); ~10 Hz
- `final-transcript-status {meeting_id, state:"queued"|"uploading"|"transcribing"|"done"|"failed", model, message|null, segments_count|null}`
- `mic-device-switched`, `mic-unavailable`, `mic-swap-failed`, `mic-recovery-exhausted`, `recording-error {message}`
- `import-progress`, `import-complete {meeting_id}`, `import-error`
- summary events as before; `voice-settings-updated`

## 10. Build and install

- GMK: `scripts/build-windows.sh` → `pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis --no-sign`.
  ffmpeg is downloaded at build time and verified by pinned SHA-256 (never executed during the build).
- CICA: `scripts/install-cica.sh` → `scp` the setup exe, silent per-user install (`/S`). The key cannot be stored over ssh
  (Credential Manager refuses a network logon session): paste it in Settings, or run `cmdkey` in the desktop session.
- Logs: `%LOCALAPPDATA%\studio.peacockery.meetily\logs\` (tauri-plugin-log), readable over `ssh cica`.

## 11. Frontend modernization (0.6.0)

Next.js 16, React 19, Tailwind 4, and BlockNote 0.57 use the static-export Tauri contract.
Turbopack builds the app with React Strict Mode enabled. BlockNote and Tiptap share their
current compatible dependencies; the former Webpack aliases and pinned ProseMirror overrides are removed.

The shadcn shell owns sidebar collapse state, persisted locally. Meeting data lives in
`contexts/MeetingContext.tsx`; `hooks/meeting-details/useSummaryPolling.ts` owns polling.
The compatibility export in `components/Sidebar/SidebarProvider.tsx` preserves existing hook imports.
New meeting opens setup; Start recording supplies the optional title and selected devices to Rust.
Tray requests use context state instead of window events and session-storage flags.

Search debounces 200 ms and invalidates prior responses immediately, including on clear and unmount.
Rust returns at most 100 distinct meeting hits, newest first, from the best transcript. SQL wildcard
characters are escaped so search terms are literal. Indexed full-text search remains a separate scaling option.

Ultracite's core, React, Next.js, anti-slop, and shadcn presets run through Oxlint. Oxfmt uses
Ultracite's formatter preset through explicit config. Project exceptions are documented inline:
component naming, native callback APIs, compiler-only checks, runtime parsers, native test fixtures,
and measured transcript virtualization. `pnpm check` runs the complete frontend gate.

The browser preview serves the static export, with an explicit desktop-capability banner.
Native recording and credentials remain in Tauri. `scripts/preview.sh up|down|status` manages both
the systemd user service and public tunnel. The preview service has no login or boot autostart.

### Scribe v3 live delivery

Scribe v3 waits for the diarized commit before saving each passage. The API preserves word times and speaker turns, and the app splits multi-speaker commits into separate lines. On a provider rejection or two transient failures, each Scribe channel switches to Nova while local audio capture continues. Nova uses the configured channel labels. Authentication failures remain actionable errors. The API allows eight seconds for delayed labels after stop. Existing settings remain preserved; the November 13 installation explicitly selects Scribe v3, enables live labels, and turns automatic final transcription off.

Automatic summaries use the saved live transcript when automatic final transcription is off. Summary registration happens before the recording-stopped event so the meeting page can attach to the active summary. An optional batch pass takes precedence when it exists.

#### 0.6.1 acceptance (November 13, 2026)

The deployed lab API passed paced Scribe v3 tests with and without diarization, a Nova fallback-route test, saved-session recovery, and optional Scribe v3 batch transcription. A 12.43-second AMI fixture returned two speaker IDs. The installed Windows app recorded the same fixture, displayed Them 1/Them 2 while recording, and retained its live rows after stop with final_status unset. Fallback selection and multi-speaker line splitting have regression coverage.

Before installation, CICA's application data and recordings were backed up. Readback verified that both existing meetings, all 39 original transcript rows, and all 15 original recording-related files remained unchanged. The installation changed exactly four voice preferences: live_model, live_diarize, final_model, and auto_final. Credentials, installation identity, summary preferences, and vocabulary remained preserved.
