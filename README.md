# Peacockery Meetily

Internal Windows meeting and call recorder for the CICA PC. It records the microphone and the PC's
sound together, shows a live transcript while recording, and produces an accurate transcript with
speaker names after the recording stops. It runs no local models: all transcription and
summaries go through the Peacockery Voice API (`https://voice-lab.peacockery.studio`).

Fork of [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes) (MIT, see `NOTICE`), cut
down to this one use.

## Using it

1. Start **Peacockery Meetily** from the Start menu.
2. First run only: **Settings → Voice** → paste the Peacockery Voice API key → **Test connection**.
   The key is kept in Windows Credential Manager.
3. Pick the microphone and the system source next to the record button:
   - the default speakers: everything the PC plays;
   - **Google Chrome app audio (Google Voice)**: only Chrome's sound, so other apps stay out of
     the recording. Chrome must be running. This captures every Chrome tab, not only Google Voice.
4. Press **Record**. The live transcript appears as you talk ("Me" is the microphone, "Them" is
   the PC sound). Pause and resume as needed.
5. Press **Stop**. The app finishes the live transcript, saves `audio.mp4`, then uploads it for the
   final transcript. When that is done the meeting page shows it in place of the live one.
6. On the meeting page: play the audio, click a line to jump to it, rename speakers, generate a
   summary, or run the final transcript again with another model.

Existing audio files can be imported from the sidebar; they get a final transcript the same way.

## Models

Chosen in **Settings → Voice**. A model is greyed out when the API does not list it.

| Use | Default | Others |
|---|---|---|
| Live transcript | Deepgram Nova-3 | Voxtral Mini Realtime 3 (optional live speaker labels), Scribe v2 Realtime, Cohere Transcribe (GMK) |
| Final transcript | ElevenLabs Scribe v2, with diarization | Voxtral Small Transcribe 3, Voxtral Mini Transcribe 3, Deepgram Nova-3, Cohere Transcribe (GMK) |
| Summary | Gemini 3.8 Flash | Claude Sonnet 5.5, DeepSeek V4 Flash, Gemini 3.1 Flash Lite, GPT-5.4 mini |

Other settings: live transcript on or off, split (mic and PC sound as separate "Me"/"Them"
streams) or mixed, language, key terms (names and companies the models should know), speaker
labels, automatic final transcript, automatic summary.

## Where things are

| What | Where |
|---|---|
| Recordings | `%USERPROFILE%\Music\Peacockery Meetily\<title>_<date>\` (`audio.mp4`, `final-transcript.json`, `final-artifact.json`, `channel-energy.json`) |
| Database and settings | `%APPDATA%\studio.peacockery.meetily\` |
| Log | `%LOCALAPPDATA%\studio.peacockery.meetily\logs\peacockery-meetily.log` |
| API key | Credential Manager, generic credential `studio.peacockery.meetily/peacockery-voice-api-key` |

## Build and install

Built on GMK (Linux) for Windows; see `ARCHITECTURE.md` §10.

```bash
scripts/build-windows.sh   # cross-compiles the NSIS installer with cargo-xwin
scripts/install-cica.sh    # copies it to CICA over ssh and installs it silently
```

The API key cannot be stored over ssh (Credential Manager refuses a network logon session). Paste
it in Settings, or run `cmdkey /generic:studio.peacockery.meetily/peacockery-voice-api-key
/user:peacockery-voice-api-key /pass:<key>` in a terminal on the PC itself.

## Development

```bash
cargo test --workspace                 # Rust unit and replay tests (Linux host)
cargo clippy --workspace --all-targets
cargo xwin check --workspace --target x86_64-pc-windows-msvc
cd frontend && pnpm install && pnpm typecheck && pnpm lint && bun test && pnpm build
```

The live-API tests in `crates/peacockery-voice` are ignored by default; run them with
`PEACOCKERY_VOICE_LAB_API_KEY` set and `cargo test -p peacockery-voice -- --ignored`.
