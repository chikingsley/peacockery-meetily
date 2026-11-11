//! Async (tokio) client for the Peacockery Voice API, as used by Peacockery Meetily.
//!
//! See `ARCHITECTURE.md` §3, §5 and §6 at the repository root for the contract.
//!
//! # Modules
//!
//! - [`client`]: [`VoiceClient::new`]`(`[`ClientConfig`]`)`, Bearer + `X-Peacockery-*` headers,
//!   typed [`Error`]s (unauthorized / forbidden / not found / bad request with body / server / network),
//!   [`VoiceClient::health`].
//! - [`catalog`]: [`VoiceClient::models`] → [`Catalog`] (tolerant serde), [`Catalog::realtime_model_id`],
//!   the curated UI lists [`curated_live`], [`curated_final`], [`curated_summary`] → [`ModelOption`],
//!   and [`ModelFamily`].
//! - [`batch`]: [`VoiceClient::transcribe_file`] (upload → job → poll, with progress and a
//!   `CancellationToken`) and the individual steps ([`VoiceClient::upload_file`],
//!   [`VoiceClient::create_transcription`], [`VoiceClient::wait_for_job`] …).
//! - [`artifact`]: [`Artifact`], the TranscriptionArtifact subset; [`Artifact::raw`] keeps the full JSON.
//! - [`turns`]: [`turns_from_artifact`] → [`Turn`]s (§6.2).
//! - [`speakers`]: [`EnergyTrack`] (`channel-energy.json`) and [`name_speakers`] (§6.3).
//! - [`realtime`]: [`RealtimeSession::connect`] → ([`RealtimeSender`], receiver of [`RealtimeEvent`]),
//!   [`RealtimeSender::send_pcm`], [`RealtimeSender::finish`] → [`RealtimeTerminal`].
//! - [`live`]: the pure [`LineBuilder`] (§5) turning events into [`LineUpdate`]s.
//! - [`text`]: [`VoiceClient::generate`] for `POST /v1/text`.
//!
//! # Example
//!
//! ```no_run
//! # async fn run() -> peacockery_voice::Result<()> {
//! use peacockery_voice::*;
//! let client = VoiceClient::new(ClientConfig::new(DEFAULT_BASE_URL, std::env::var("KEY").unwrap()))?;
//! let catalog = client.models().await?;
//! let model = catalog.realtime_model_id("mistral-voxtral-mini-transcribe-realtime-3").unwrap();
//! let (mut sender, mut events) = RealtimeSession::connect(&client, RealtimeOptions::new(model)).await?;
//! let mut lines = LineBuilder::new(LineBuilderConfig::new(Channel::Mic, model, SpeakerLabels::Fixed("Me".into())));
//! sender.send_pcm(&[0i16; FRAME_SAMPLES]).await?;
//! while let Ok(event) = events.try_recv() {
//!     for update in lines.on_event(&event, sender.seconds_sent(SAMPLE_RATE)) {
//!         println!("{update:?}");
//!     }
//! }
//! let terminal = sender.finish(DEFAULT_FINISH_TIMEOUT).await?;
//! # let _ = terminal; Ok(()) }
//! ```

pub mod artifact;
pub mod batch;
pub mod catalog;
pub mod client;
pub mod error;
pub mod live;
pub mod realtime;
pub mod speakers;
pub mod text;
pub mod turns;

pub use artifact::{Artifact, Content, LanguageInfo, Metrics, Provenance, TimedText, Track};
pub use batch::{
    content_type_for, BatchProgress, BatchTranscript, CompletedPart, CompletedUpload, JobStatus,
    JobView, ProgressFn, TranscribeOptions, Transfer, TransferPart, UploadReservation,
};
pub use catalog::{
    curated_final, curated_live, curated_summary, Catalog, CatalogModel, ModelFamily, ModelOption,
    Route, Routes,
};
pub use client::{mask_key, ClientConfig, Health, VoiceClient, DEFAULT_BASE_URL};
pub use error::{Error, Result};
pub use live::{
    Channel, LineBuilder, LineBuilderConfig, LineStyle, LineUpdate, LiveMode, SpeakerLabels,
};
pub use realtime::{
    LiveSegment, RealtimeEvent, RealtimeOptions, RealtimeSender, RealtimeSession, RealtimeTerminal,
    SessionError, TranscriptEvent, DEFAULT_FINISH_TIMEOUT, FRAME_SAMPLES,
    SAMPLE_RATE,
};
pub use speakers::{name_speakers, rms_dbfs, EnergyTrack};
pub use text::{Message, Role, Routing, TextContext, TextRequest, TextResponse, TextUsage};
pub use tokio_util::sync::CancellationToken;
pub use turns::{turns_from_artifact, turns_with_source, Turn, TurnSource};
