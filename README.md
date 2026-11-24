# Peacockery Meetily

Internal Windows meeting and call recorder for the CICA PC. It records the microphone and the PC's sound together, shows a live transcript while recording, and produces an accurate transcript with speaker names after the recording stops. It runs no local models: all transcription and summaries go through the Peacockery Voice API (`https://voice-lab.peacockery.studio`).

Fork of [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes) (MIT, see `NOTICE`), cut down to this one use.

## Using it

1. Start **Peacockery Meetily** from the Start menu.
1. First run only: enter your eight-digit activation code. **Settings → Voice → Access** also accepts it. Personal access is kept in Windows Credential Manager.
1. Under **Recording options**, pick the microphone and the call and system audio source. They start from the devices saved in Settings; **Default microphone** and **Default playback device** follow Windows. For call audio:
   - **Default playback device**: everything the PC plays;
   - **Google Chrome app audio (Google Voice)**: only Chrome's sound, so other apps stay out of the recording. Chrome must be running. This captures every Chrome tab, not only Google Voice.
1. Choose **In person**, **Online meeting** or **Phone call** before recording. In person captures the microphone only and separates voices in the room. Online and phone capture the microphone and PC audio, with speaker separation on both channels. For a separate phone on speaker beside the microphone, choose In person.
1. Choose an event from **Today’s calendar**, or leave **Unscheduled meeting** and optionally enter a title. A physical room suggests In person even if the invitation contains a Teams link. You can override the suggestion. Start recording; pause and resume as needed.
1. Press **Stop**. The app finishes the live transcript and opens the meeting; `audio.mp4` is written in the background (the meeting page shows **Saving audio…** until it is ready). A live transcript that ran completely on Scribe v3 is the result (**Live · Scribe v3 · complete**). Otherwise the audio is uploaded for the Scribe v3 final transcript, which then replaces the live one on the meeting page.
1. On the meeting page: play the audio, click a line to jump to it, rename speakers, generate a summary, or run the final transcript again with another model.

Existing audio files can be imported from the sidebar; they get a final transcript the same way.

## Models

Chosen in **Settings → Voice**. A model is greyed out when the API does not list it.

| Use              | Default                                | Others                                                                                          |
| ---------------- | -------------------------------------- | ----------------------------------------------------------------------------------------------- |
| Live transcript  | Scribe v3                              | Nova-3 fallback; Voxtral Mini Realtime 3, Cohere Transcribe (GMK)                               |
| Final transcript | ElevenLabs Scribe v3, with diarization | Voxtral Small Transcribe 3, Voxtral Mini Transcribe 3, Deepgram Nova-3, Cohere Transcribe (GMK) |
| Summary          | Gemini 3.8 Flash                       | Claude Sonnet 5.5, DeepSeek V4 Flash, Gemini 3.1 Flash Lite, GPT-5.4 mini                       |

Other settings: live transcript on or off, language, key terms, recognition corrections for the final transcript, final speaker labels, when the final transcript runs after stop (by default only when the live transcript was incomplete or not on Scribe v3; or always, or never), and automatic summary. Live labels number the voices on each channel. A microphone is not a person, and invitations do not establish speaker identity.

## Where things are

| What                  | Where                                                                                                                                         |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Recordings            | `%USERPROFILE%\Music\Peacockery Meetily\<title>_<date>\` (`audio.mp4`, `final-transcript.json`, `final-artifact.json`) |
| Database and settings | `%APPDATA%\studio.peacockery.meetily\`                                                                                                        |
| Log                   | `%LOCALAPPDATA%\studio.peacockery.meetily\logs\peacockery-meetily.log`                                                                        |
| API key               | Credential Manager, generic credential `studio.peacockery.meetily/peacockery-voice-api-key`                                                   |

## Calendar and capture context

Connect your own work Outlook account through **Settings → General → Outlook calendar**, complete Microsoft's sign-in in the dedicated Edge window, and choose a calendar. Meetily reads nearby events through the same Outlook Web route as the CICA collector. Windows protects the session on your PC, and the saved browser session supplies renewed access automatically. When Microsoft asks for a fresh sign-in, Settings shows the error and **Connect Outlook** signs in again. **Disconnect** clears the selected calendar and removes the local session and browser profile.

Recording also works with an unscheduled meeting. Calendar context is optional and read-only.

Each recording saves its chosen mode and the selected calendar occurrence, organizer and invitees in `recording-context.json` beside its audio. These remain unchanged when settings change later. Final transcripts keep numbered speakers.

## Build and install

Built on GMK (Linux) for Windows; see `ARCHITECTURE.md` §10.

```bash
scripts/build-windows.sh   # cross-compiles the NSIS installer with cargo-xwin
scripts/install-cica.sh    # copies it to CICA over ssh and installs it silently
```

Activation runs in the signed-in Windows desktop session and stores the credential there. Give coworkers their activation code and the installer link; operator credential records stay in the private operator store.

## Development

```bash
cargo test --workspace                 # Rust unit and replay tests (Linux host)
cargo clippy --workspace --all-targets
cargo xwin check --workspace --target x86_64-pc-windows-msvc
cd frontend && pnpm install --frozen-lockfile && pnpm check
```

The live-API tests in `crates/peacockery-voice` are ignored by default; run them with `PEACOCKERY_VOICE_LAB_API_KEY` set and `cargo test -p peacockery-voice -- --ignored`.

## UI development

The UI uses Next.js 16, React 19, Tailwind 4 and shadcn components. General settings include light, dark and system themes. Use `pnpm format` to apply the Ultracite/Oxfmt preset and `pnpm lint` for the Oxlint core, React, Next.js, anti-slop and shadcn checks.

## Sharing and updating

See [Distribution and updates](DISTRIBUTION.md) for the download link, per-person activation, signed release workflow and calendar prerequisites.

### Meeting prep and call review

On the managed CICA installation, **Calls and meeting prep** shows the signed-in calendar's upcoming week and Chi’s assigned CRM call tasks. Select an item to edit its private prep. Start call starts Phone recording and opens the task number in Google Voice in Chrome. The installation uses Google account slot zero by default. On PCs with multiple Google accounts, the top-level integer `google_voice_account` in `recording_preferences.json` selects the slot from the signed-in Voice URL. The same prep follows the recording. Each automatic speaker name carries a source-specific introduction. Manual labels take precedence. The recording detail lists call outcomes and their timestamps.

On GMK, run `node scripts/sync-cica-agenda.mjs --list` to see the available item keys and source links. Read an item with `--read-prep KEY`. Pipe the prepared text to `--save-prep KEY VERSION`, using the returned version to protect concurrent edits. All three commands target the same prep store as the desktop editor. Include the supplier role, exact part numbers, drawing revision and detail, RFP paragraph, source links, and outstanding questionnaire items when those records support them. The selected calendar and Windows destination are bound in the managed agenda command. Other installations use their own local prep store.

The CICA agenda service also stages named transcripts from explicitly published recordings in the portal’s existing meeting folders. Desktop publication checks import the website's minutes and action items. Editing a local summary gives that copy precedence.

Agents can also read and edit saved recordings with the same command. Use `--list-recordings` for IDs and `--read-recording ID` for the title, Markdown minutes, source-specific speaker labels and current version. Pipe a JSON object to `--edit-recording ID VERSION`. Supported fields are `title`, `markdown` and `speaker` (an object with `source`, `label` and `name`). Each edit checks the version, applies all fields together and returns the saved record. Editing minutes saves local notes. The command invokes no text model.
