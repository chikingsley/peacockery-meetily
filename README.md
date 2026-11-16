# Peacockery Meetily

Internal Windows meeting and call recorder for the CICA PC. It records the microphone and the PC's sound together, shows a live transcript while recording, and produces an accurate transcript with speaker names after the recording stops. It runs no local models: all transcription and summaries go through the Peacockery Voice API (`https://voice-lab.peacockery.studio`).

Fork of [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes) (MIT, see `NOTICE`), cut down to this one use.

## Using it

1. Start **Peacockery Meetily** from the Start menu.
1. First run only: **Settings → Voice → Activate access** and choose your private activation file. Access is kept in Windows Credential Manager. Advanced connection settings remain available for operators.
1. Pick the microphone and the system source next to the record button:
   - the default speakers: everything the PC plays;
   - **Google Chrome app audio (Google Voice)**: only Chrome's sound, so other apps stay out of the recording. Chrome must be running. This captures every Chrome tab, not only Google Voice.
1. Choose **In person**, **Online meeting** or **Phone call** before recording. In person captures the microphone only and separates voices in the room. Online and phone capture the microphone and PC audio, with speaker separation on both channels. For a separate phone on speaker beside the microphone, choose In person.
1. Choose an event from **Today’s calendar**, or leave **Unscheduled meeting** and enter a title. A physical room suggests In person even if the invitation contains a Teams link. You can override the suggestion. Start recording; pause and resume as needed.
1. Press **Stop**. The app finishes the live transcript, saves `audio.mp4`, then uploads it for the final transcript. When that is done the meeting page shows it in place of the live one.
1. On the meeting page: play the audio, click a line to jump to it, rename speakers, generate a summary, or run the final transcript again with another model.

Existing audio files can be imported from the sidebar; they get a final transcript the same way.

## Models

Chosen in **Settings → Voice**. A model is greyed out when the API does not list it.

| Use              | Default                                | Others                                                                                          |
| ---------------- | -------------------------------------- | ----------------------------------------------------------------------------------------------- |
| Live transcript  | Scribe v3                              | Nova-3 fallback; Voxtral Mini Realtime 3, Scribe v2 Realtime, Cohere Transcribe (GMK)           |
| Final transcript | ElevenLabs Scribe v3, with diarization | Voxtral Small Transcribe 3, Voxtral Mini Transcribe 3, Deepgram Nova-3, Cohere Transcribe (GMK) |
| Summary          | Gemini 3.8 Flash                       | Claude Sonnet 5.5, DeepSeek V4 Flash, Gemini 3.1 Flash Lite, GPT-5.4 mini                       |

Other settings: live transcript on or off, language, key terms, final speaker labels, automatic final transcript (on by default), and automatic summary. Live labels are provisional. A microphone is not a person, and invitations do not establish speaker identity.

## Where things are

| What                  | Where                                                                                                                                         |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Recordings            | `%USERPROFILE%\Music\Peacockery Meetily\<title>_<date>\` (`audio.mp4`, `final-transcript.json`, `final-artifact.json`, `channel-energy.json`) |
| Database and settings | `%APPDATA%\studio.peacockery.meetily\`                                                                                                        |
| Log                   | `%LOCALAPPDATA%\studio.peacockery.meetily\logs\peacockery-meetily.log`                                                                        |
| API key               | Credential Manager, generic credential `studio.peacockery.meetily/peacockery-voice-api-key`                                                   |

## Calendar and capture context

The existing GMK CICA Outlook collector projects a private, read-only calendar snapshot to `%APPDATA%\studio.peacockery.meetily\calendar-events.json`. It uses the primary work calendar, includes yesterday through tomorrow, and refreshes at least every five minutes while CICA is reachable. The app shows a stale-copy notice after 15 minutes and remains able to record without a calendar. No new calendar credentials or scheduled worker run on CICA.

Each recording saves its chosen mode and the selected calendar occurrence, organizer and invitees in `recording-context.json` beside its audio. These remain unchanged when settings change later. Final transcripts for these recordings keep numbered speakers rather than assigning a person from microphone energy.

## Build and install

Built on GMK (Linux) for Windows; see `ARCHITECTURE.md` §10.

```bash
scripts/build-windows.sh   # cross-compiles the NSIS installer with cargo-xwin
scripts/install-cica.sh    # copies it to CICA over ssh and installs it silently
```

The API key cannot be stored over ssh (Credential Manager refuses a network logon session). Paste it in Settings, or run `cmdkey /generic:studio.peacockery.meetily/peacockery-voice-api-key /user:peacockery-voice-api-key /pass:<key>` in a terminal on the PC itself.

## Development

```bash
cargo test --workspace                 # Rust unit and replay tests (Linux host)
cargo clippy --workspace --all-targets
cargo xwin check --workspace --target x86_64-pc-windows-msvc
cd frontend && pnpm install --frozen-lockfile && pnpm check
```

The live-API tests in `crates/peacockery-voice` are ignored by default; run them with `PEACOCKERY_VOICE_LAB_API_KEY` set and `cargo test -p peacockery-voice -- --ignored`.

## UI development and preview

The UI uses Next.js 16, React 19, Tailwind 4 and shadcn components. General settings include light, dark and system themes. Use `pnpm format` to apply the Ultracite/Oxfmt preset and `pnpm lint` for the Oxlint core, React, Next.js, anti-slop and shadcn checks.

After `pnpm build`, run `scripts/preview.sh up` from the repository root for the supervised browser preview. Use `scripts/preview.sh status` to inspect it and `scripts/preview.sh down` to stop it and remove its tunnel. The browser preview shows UI; recording runs in the desktop app.

## Sharing and updating

See [Distribution and updates](DISTRIBUTION.md) for the download link, per-person activation, signed release workflow and calendar prerequisites.
