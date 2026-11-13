//! `/v1/realtime` WebSocket: options → query, typed events, PCM sender, close handshake.

use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::artifact::{de_speaker, Artifact};
use crate::catalog::ModelFamily;
use crate::client::{ensure_crypto_provider, VoiceClient, HEADER_CLIENT_SESSION_ID};
use crate::error::{Error, Result};

/// Audio sample rate the app streams.
pub const SAMPLE_RATE: u32 = 16_000;
/// Samples in one 100 ms frame at 16 kHz.
pub const FRAME_SAMPLES: usize = 1_600;
/// How long `finish` waits for the terminal event by default.
pub const DEFAULT_FINISH_TIMEOUT: Duration = Duration::from_secs(20);
/// Buffered events between the socket reader and the consumer.
const EVENT_BUFFER: usize = 4096;



/// Options for one realtime session.
#[derive(Debug, Clone, PartialEq)]
pub struct RealtimeOptions {
    /// WebSocket model id (`routes.realtime.model`, see [`crate::Catalog::realtime_model_id`]).
    pub model: String,
    pub sample_rate: u32,
    /// ISO-639-1 code; `None` or `"auto"` lets the provider detect.
    pub language: Option<String>,
    /// Speaker labels on the stream (Voxtral realtime 3, Deepgram). Rejected for Scribe, which fails the session.
    pub diarize: bool,
    /// Voxtral realtime 3 only: 80–1200 or 2400 ms.
    pub target_streaming_delay_ms: Option<u32>,
    /// Vocabulary hints. Sent as `keyterm` on Deepgram/Scribe and merged into `context_bias` on Voxtral 3.
    pub keyterms: Vec<String>,
    /// Voxtral realtime 3 only.
    pub context_bias: Vec<String>,

    /// `X-Peacockery-Client-Session-ID` (the meeting id).
    pub client_session_id: Option<String>,
}

impl RealtimeOptions {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            sample_rate: SAMPLE_RATE,
            language: Some("en".into()),
            diarize: false,
            target_streaming_delay_ms: None,
            keyterms: Vec::new(),
            context_bias: Vec::new(),
            client_session_id: None,
        }
    }

    /// Query pairs valid for this model family, in a stable order. Options the family does not
    /// support are left out; contradictory options are an error.
    pub fn query_pairs(&self) -> Result<Vec<(&'static str, String)>> {
        let family = ModelFamily::of(&self.model);
        if self.model.trim().is_empty() {
            return Err(Error::Config("realtime model is empty".into()));
        }
        if self.sample_rate == 0 {
            return Err(Error::Config("sample_rate must be positive".into()));
        }
        let mut pairs = vec![
            ("model", self.model.clone()),
            ("encoding", "linear16".to_string()),
            ("sample_rate", self.sample_rate.to_string()),
        ];
        if let Some(lang) = self
            .language
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty() && *l != "auto")
        {
            pairs.push(("language", lang.to_string()));
        }
        if self.diarize {
            if family == ModelFamily::Cohere || (family == ModelFamily::Scribe && !self.model.ends_with("scribe_v3")) {
                return Err(Error::Config(format!(
                    "{} does not support live speaker labels",
                    self.model
                )));
            }
            pairs.push(("diarize", "true".into()));
        }
        let terms = |list: &[String]| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for t in list.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
                if !out.iter().any(|o| o == t) {
                    out.push(t.to_string());
                }
            }
            out
        };
        match family {
            ModelFamily::VoxtralRealtime3 => {
                if let Some(delay) = self.target_streaming_delay_ms {
                    if !((80..=1200).contains(&delay) || delay == 2400) {
                        return Err(Error::Config(format!(
                            "target_streaming_delay_ms {delay} must be 80–1200 or 2400"
                        )));
                    }
                    pairs.push(("target_streaming_delay_ms", delay.to_string()));
                }
                for term in terms(&[self.context_bias.clone(), self.keyterms.clone()].concat()) {
                    pairs.push(("context_bias", term));
                }

            }
            ModelFamily::Deepgram | ModelFamily::Scribe => {
                for term in terms(&self.keyterms) {
                    pairs.push(("keyterm", term));
                }
            }
            ModelFamily::Mistral | ModelFamily::Cohere | ModelFamily::Other => {}
        }
        Ok(pairs)
    }

    /// Full `wss://…/v1/realtime?…` URL.
    pub fn url(&self, client: &VoiceClient) -> Result<Url> {
        let mut url = client.ws_url("v1/realtime")?;
        url.query_pairs_mut().extend_pairs(self.query_pairs()?);
        Ok(url)
    }
}

/// A word, segment or speaker turn inside a live transcript event (camelCase on the wire).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSegment {
    #[serde(default)]
    pub start_seconds: Option<f64>,
    #[serde(default)]
    pub end_seconds: Option<f64>,
    #[serde(default)]
    pub text: String,
    #[serde(default, deserialize_with = "de_speaker")]
    pub speaker: Option<String>,
}

/// Payload of `transcript.interim` / `transcript.delta` / `transcript.committed`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TranscriptEvent {
    #[serde(default)]
    pub sequence: u64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub segments: Vec<LiveSegment>,
    #[serde(default)]
    pub speaker_turns: Vec<LiveSegment>,
    #[serde(default)]
    pub words: Vec<LiveSegment>,
    #[serde(default)]
    pub speech_final: Option<bool>,
}

/// `session.failed.error`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionError {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
}

/// Every server event of protocol version 1. Unknown types are preserved as [`RealtimeEvent::Other`].
#[derive(Debug, Clone, PartialEq)]
pub enum RealtimeEvent {
    SessionStarted {
        session_id: String,
        model: String,
        language: Option<String>,
    },
    /// One per binary frame received by the server.
    AudioReceived {
        audio_bytes: u64,
        chunk_bytes: u64,
        message_count: u64,
    },
    /// Full current hypothesis (Deepgram, Scribe): replaces the preview.
    Interim(TranscriptEvent),
    /// Text fragment to append verbatim (Voxtral).
    Delta(TranscriptEvent),
    /// Final segment (Deepgram `is_final`, Scribe VAD commit, Voxtral diarized fragment).
    Committed(TranscriptEvent),
    TurnStart {
        time_seconds: f64,
    },
    TurnEagerEnd {
        time_seconds: f64,
    },
    TurnResume {
        time_seconds: f64,
    },
    TurnEnd {
        time_seconds: f64,
        end_reason: Option<String>,
    },
    SessionCompleted {
        session_id: String,
        result: Box<Artifact>,
    },
    SessionFailed {
        session_id: String,
        error: SessionError,
        result: Option<Box<Artifact>>,
    },
    Pong,
    /// Echo of a text frame the server did not understand as control.
    TextReceived(Value),
    /// Echo of an unknown JSON control message.
    EventReceived(Value),
    Other {
        kind: String,
        raw: Value,
    },
}

impl RealtimeEvent {
    /// Parse one text frame.
    pub fn parse(text: &str) -> Result<Self> {
        Self::from_value(serde_json::from_str(text)?)
    }

    /// Parse one event object.
    pub fn from_value(raw: Value) -> Result<Self> {
        let kind = raw
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let str_field = |k: &str| {
            raw.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let num = |k: &str| raw.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let int = |k: &str| raw.get(k).and_then(Value::as_u64).unwrap_or(0);
        let transcript = || TranscriptEvent::deserialize(&raw);
        let artifact = |v: Option<&Value>| -> Result<Option<Box<Artifact>>> {
            match v {
                Some(v) if v.is_object() => Ok(Some(Box::new(Artifact::from_value(v.clone())?))),
                _ => Ok(None),
            }
        };
        Ok(match kind.as_str() {
            "session.started" => RealtimeEvent::SessionStarted {
                session_id: str_field("session_id"),
                model: str_field("model"),
                language: raw
                    .get("language")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            },
            "audio.received" => RealtimeEvent::AudioReceived {
                audio_bytes: int("audio_bytes"),
                chunk_bytes: int("chunk_bytes"),
                message_count: int("message_count"),
            },
            "transcript.interim" => RealtimeEvent::Interim(transcript()?),
            "transcript.delta" => RealtimeEvent::Delta(transcript()?),
            "transcript.committed" => RealtimeEvent::Committed(transcript()?),
            "turn.start" => RealtimeEvent::TurnStart {
                time_seconds: num("time_seconds"),
            },
            "turn.eager_end" => RealtimeEvent::TurnEagerEnd {
                time_seconds: num("time_seconds"),
            },
            "turn.resume" => RealtimeEvent::TurnResume {
                time_seconds: num("time_seconds"),
            },
            "turn.end" => RealtimeEvent::TurnEnd {
                time_seconds: num("time_seconds"),
                end_reason: raw
                    .get("end_reason")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            },
            "session.completed" => RealtimeEvent::SessionCompleted {
                session_id: str_field("session_id"),
                result: artifact(raw.get("result"))?
                    .ok_or_else(|| Error::Decode("session.completed without result".into()))?,
            },
            "session.failed" => RealtimeEvent::SessionFailed {
                session_id: str_field("session_id"),
                error: raw
                    .get("error")
                    .map(SessionError::deserialize)
                    .transpose()?
                    .unwrap_or_default(),
                result: artifact(raw.get("result"))?,
            },
            "pong" => RealtimeEvent::Pong,
            "text.received" => RealtimeEvent::TextReceived(raw),
            "event.received" => RealtimeEvent::EventReceived(raw),
            _ => RealtimeEvent::Other { kind, raw },
        })
    }


}

/// How a session ended.
#[derive(Debug, Clone, PartialEq)]
pub enum RealtimeTerminal {
    Completed {
        session_id: String,
        result: Box<Artifact>,
    },
    Failed {
        session_id: String,
        error: SessionError,
        result: Option<Box<Artifact>>,
    },
}

impl RealtimeTerminal {
    pub fn session_id(&self) -> &str {
        match self {
            RealtimeTerminal::Completed { session_id, .. }
            | RealtimeTerminal::Failed { session_id, .. } => session_id,
        }
    }

    /// The provider's artifact, when the server sent one.
    pub fn result(&self) -> Option<&Artifact> {
        match self {
            RealtimeTerminal::Completed { result, .. } => Some(result),
            RealtimeTerminal::Failed { result, .. } => result.as_deref(),
        }
    }
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Write half of a realtime session.
pub struct RealtimeSender {
    sink: SplitSink<Socket, Message>,
    terminal: Option<oneshot::Receiver<Result<RealtimeTerminal>>>,
    /// Terminal outcome picked up early (after a failed send), handed out by `finish`.
    ended: Option<Result<RealtimeTerminal>>,
    bytes_sent: u64,
}

impl std::fmt::Debug for RealtimeSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealtimeSender")
            .field("bytes_sent", &self.bytes_sent)
            .finish()
    }
}

/// An open realtime session.
pub struct RealtimeSession;

impl RealtimeSession {
    /// Open the WebSocket. Returns the PCM sender and the event stream (every event, including
    /// `audio.received` and the terminal one). HTTP rejections of the upgrade map to the usual
    /// [`Error`] variants with the response body (400 bad option, 401 key, 403 scope).
    pub async fn connect(
        client: &VoiceClient,
        options: RealtimeOptions,
    ) -> Result<(RealtimeSender, mpsc::Receiver<RealtimeEvent>)> {
        ensure_crypto_provider();
        let url = options.url(client)?;
        let mut request = url.as_str().into_client_request().map_err(ws_error)?;
        let headers = request.headers_mut();
        for (name, value) in client.auth_headers()? {
            headers.insert(name, value);
        }
        if let Some(id) = options
            .client_session_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            let value = HeaderValue::from_str(id.trim()).map_err(|_| {
                Error::Config("client_session_id is not a valid header value".into())
            })?;
            headers.insert(HEADER_CLIENT_SESSION_ID, value);
        }
        let connect = tokio_tungstenite::connect_async(request);
        let (socket, _response) = tokio::time::timeout(Duration::from_secs(20), connect)
            .await
            .map_err(|_| Error::Timeout("realtime connect".into()))?
            .map_err(ws_error)?;
        let (sink, stream) = socket.split();
        let (events_tx, events_rx) = mpsc::channel(EVENT_BUFFER);
        let (terminal_tx, terminal_rx) = oneshot::channel();
        tokio::spawn(read_loop(stream, events_tx, terminal_tx));
        Ok((
            RealtimeSender {
                sink,
                terminal: Some(terminal_rx),
                ended: None,
                bytes_sent: 0,
            },
            events_rx,
        ))
    }
}

async fn read_loop(
    mut stream: SplitStream<Socket>,
    events: mpsc::Sender<RealtimeEvent>,
    terminal: oneshot::Sender<Result<RealtimeTerminal>>,
) {
    let mut terminal = Some(terminal);
    let mut close_reason = String::from("socket ended");
    while let Some(message) = stream.next().await {
        let text = match message {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(frame)) => {
                close_reason = describe_close(frame.as_ref());
                break;
            }
            Ok(_) => continue,
            Err(err) => {
                close_reason = err.to_string();
                break;
            }
        };
        let event = match RealtimeEvent::parse(text.as_str()) {
            Ok(event) => event,
            Err(err) => {
                tracing::warn!(error = %err, "unparseable realtime event");
                continue;
            }
        };
        let finished = match &event {
            RealtimeEvent::SessionCompleted { session_id, result } => {
                Some(RealtimeTerminal::Completed {
                    session_id: session_id.clone(),
                    result: result.clone(),
                })
            }
            RealtimeEvent::SessionFailed {
                session_id,
                error,
                result,
            } => Some(RealtimeTerminal::Failed {
                session_id: session_id.clone(),
                error: error.clone(),
                result: result.clone(),
            }),
            _ => None,
        };
        // A dropped receiver must not stop us from seeing the terminal event.
        let _ = events.send(event).await;
        if let Some(done) = finished {
            if let Some(tx) = terminal.take() {
                let _ = tx.send(Ok(done));
            }
        }
    }
    if let Some(tx) = terminal.take() {
        let _ = tx.send(Err(Error::RealtimeClosed(close_reason)));
    }
}

fn describe_close(frame: Option<&CloseFrame>) -> String {
    match frame {
        Some(f) => format!("close {} {}", u16::from(f.code), f.reason),
        None => "close without frame".into(),
    }
}

fn ws_error(err: tungstenite::Error) -> Error {
    match err {
        tungstenite::Error::Http(response) => {
            let status = response.status().as_u16();
            let body = response
                .body()
                .as_ref()
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default();
            Error::from_status(status, body)
        }
        tungstenite::Error::Url(e) => Error::Config(format!("realtime url: {e}")),
        other => Error::Network(other.to_string()),
    }
}

impl RealtimeSender {
    /// Send mono s16le samples as one binary frame (send 100 ms = 1600 samples at a time, silence included).
    pub async fn send_pcm(&mut self, samples: &[i16]) -> Result<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let len = bytes.len() as u64;
        match self.sink.send(Message::Binary(bytes.into())).await {
            Ok(()) => {
                self.bytes_sent += len;
                Ok(())
            }
            Err(err) => Err(self.send_failure(err).await),
        }
    }

    /// A send failed: prefer the server's own reason when the session already ended.
    async fn send_failure(&mut self, err: tungstenite::Error) -> Error {
        if self.ended.is_none() {
            if let Some(rx) = self.terminal.take() {
                // The reader reports the terminal event or the close reason right after the socket ends.
                match tokio::time::timeout(Duration::from_secs(2), rx).await {
                    Ok(Ok(outcome)) => self.ended = Some(outcome),
                    _ => return ws_error(err),
                }
            }
        }
        match &self.ended {
            Some(Ok(RealtimeTerminal::Failed { error, .. })) => Error::SessionFailed {
                code: error.code.clone(),
                message: error.message.clone(),
                retryable: error.retryable,
            },
            Some(Ok(RealtimeTerminal::Completed { .. })) => {
                Error::RealtimeClosed("session already completed".into())
            }
            Some(Err(Error::RealtimeClosed(reason))) => Error::RealtimeClosed(reason.clone()),
            _ => ws_error(err),
        }
    }

    /// Audio bytes sent so far.
    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent
    }

    /// Seconds of audio sent so far at `sample_rate`.
    pub fn seconds_sent(&self, sample_rate: u32) -> f64 {
        self.bytes_sent as f64 / 2.0 / f64::from(sample_rate.max(1))
    }



    /// Send `{"type":"close"}` and wait for `session.completed` / `session.failed`.
    /// The server closes the socket afterwards; the event stream ends.
    /// A session that already ended returns its terminal outcome without sending anything.
    pub async fn finish(mut self, timeout: Duration) -> Result<RealtimeTerminal> {
        if let Some(ended) = self.ended.take() {
            let _ = self.sink.close().await;
            return ended;
        }
        let terminal = self
            .terminal
            .take()
            .ok_or_else(|| Error::RealtimeClosed("reader stopped".into()))?;
        // If the socket already died, the terminal channel carries the reason.
        let sent = self
            .sink
            .send(Message::Text(r#"{"type":"close"}"#.into()))
            .await;
        let outcome = tokio::time::timeout(timeout, terminal).await;
        let result = match outcome {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::RealtimeClosed("reader stopped".into())),
            Err(_) => Err(Error::Timeout(format!(
                "no session.completed within {} s",
                timeout.as_secs_f32()
            ))),
        };
        if result.is_err() {
            if let Err(err) = sent {
                return Err(ws_error(err));
            }
        }
        let _ = self.sink.close().await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ClientConfig;

    fn client() -> VoiceClient {
        VoiceClient::new(ClientConfig::new(
            "https://voice-lab.peacockery.studio",
            "pv_lab_test",
        ))
        .unwrap()
    }

    fn keys(o: &RealtimeOptions) -> Vec<String> {
        o.query_pairs()
            .unwrap()
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect()
    }

    #[test]
    fn voxtral3_query() {
        let mut o = RealtimeOptions::new("mistral-voxtral-mini-transcribe-realtime-3");
        o.target_streaming_delay_ms = Some(480);
        o.keyterms = vec!["Cica-Huntek".into(), " ".into(), "Netask".into()];
        o.context_bias = vec!["Netask".into()];

        assert_eq!(
            keys(&o),
            [
                "model=mistral-voxtral-mini-transcribe-realtime-3",
                "encoding=linear16",
                "sample_rate=16000",
                "language=en",
                "target_streaming_delay_ms=480",
                "context_bias=Netask",
                "context_bias=Cica-Huntek",
            ]
        );
        let url = o.url(&client()).unwrap();
        assert!(url
            .as_str()
            .starts_with("wss://voice-lab.peacockery.studio/v1/realtime?model=mistral-voxtral"));
        assert!(url
            .as_str()
            .contains("context_bias=Netask&context_bias=Cica-Huntek"));

        o.diarize = true;
        assert!(keys(&o).contains(&"diarize=true".to_string()));
        o.target_streaming_delay_ms = Some(1500);
        assert!(matches!(o.query_pairs(), Err(Error::Config(_))));
        o.target_streaming_delay_ms = Some(2400);
        assert!(o.query_pairs().is_ok());
    }

    #[test]
    fn other_families_drop_voxtral_options() {
        for model in ["deepgram-nova-3", "elevenlabs-scribe_v2"] {
            let mut o = RealtimeOptions::new(model);
            o.target_streaming_delay_ms = Some(480);
            o.context_bias = vec!["X".into()];
            o.keyterms = vec!["Netask".into(), "Netask".into()];

            o.language = Some("auto".into());
            assert_eq!(
                keys(&o),
                [
                    format!("model={model}"),
                    "encoding=linear16".into(),
                    "sample_rate=16000".into(),
                    "keyterm=Netask".into()
                ]
            );
        }
        let mut o = RealtimeOptions::new("cohere-transcribe");
        o.keyterms = vec!["Netask".into()];
        o.context_bias = vec!["X".into()];
        assert_eq!(keys(&o).len(), 4);
        let mut o = RealtimeOptions::new("mistral-voxtral-mini-transcribe-realtime-2602");
        o.context_bias = vec!["X".into()];
        o.target_streaming_delay_ms = Some(480);
        assert_eq!(keys(&o).len(), 4);
        o.diarize = true;
        assert_eq!(keys(&o).len(), 5);
    }

    #[test]
    fn diarize_rejected_where_it_fails() {
        let mut v3 = RealtimeOptions::new("elevenlabs-scribe_v3");
        v3.diarize = true;
        assert!(keys(&v3).contains(&"diarize=true".to_string()));
        let mut o = RealtimeOptions::new("elevenlabs-scribe_v2");
        o.diarize = true;
        assert!(matches!(o.query_pairs(), Err(Error::Config(_))));
        let mut o = RealtimeOptions::new("deepgram-nova-3");
        o.diarize = true;
        assert!(keys(&o).contains(&"diarize=true".to_string()));
        assert!(matches!(
            RealtimeOptions::new(" ").query_pairs(),
            Err(Error::Config(_))
        ));
    }

    fn fixture(name: &str) -> Vec<Value> {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_every_fixture_event() {
        for name in [
            "vox-plain.json",
            "vox-turn.json",
            "vox-diar.json",
            "nova.json",
            "scribe-rt.json",
        ] {
            let events: Vec<_> = fixture(name)
                .into_iter()
                .map(|v| RealtimeEvent::from_value(v).unwrap())
                .collect();
            assert!(
                matches!(&events[0], RealtimeEvent::SessionStarted { session_id, .. } if session_id.starts_with("rt_")),
                "{name}"
            );
            assert!(
                matches!(events.last().unwrap(), RealtimeEvent::SessionCompleted { result, .. } if !result.text.is_empty()),
                "{name}"
            );
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e, RealtimeEvent::Other { .. })),
                "{name}"
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|e| matches!(
                        e,
                        RealtimeEvent::SessionCompleted { .. } | RealtimeEvent::SessionFailed { .. }
                    ))
                    .count(),
                1
            );
        }
        let diar: Vec<_> = fixture("vox-diar.json")
            .into_iter()
            .map(|v| RealtimeEvent::from_value(v).unwrap())
            .collect();
        let RealtimeEvent::Committed(c) = &diar[1] else {
            panic!("{:?}", diar[1])
        };
        assert_eq!(c.segments[0].speaker.as_deref(), Some("speaker_1"));
        assert_eq!(c.segments[0].start_seconds, None);
        let nova: Vec<_> = fixture("nova.json")
            .into_iter()
            .map(|v| RealtimeEvent::from_value(v).unwrap())
            .collect();
        let RealtimeEvent::Committed(c) = &nova[1] else {
            panic!()
        };
        assert_eq!(
            (c.segments[0].start_seconds, c.segments[0].end_seconds),
            (Some(0.0), Some(0.53))
        );
        let turn: Vec<_> = fixture("vox-turn.json")
            .into_iter()
            .map(|v| RealtimeEvent::from_value(v).unwrap())
            .collect();
        assert!(turn
            .iter()
            .any(|e| matches!(e, RealtimeEvent::TurnEnd { .. })));
        assert!(turn
            .iter()
            .any(|e| matches!(e, RealtimeEvent::TurnEagerEnd { .. })));
    }

    #[test]
    fn parses_control_and_failure_events() {
        let e = RealtimeEvent::parse(r#"{"type":"audio.received","audio_bytes":6400,"chunk_bytes":3200,"message_count":2,"session_id":"rt_x"}"#).unwrap();
        assert_eq!(
            e,
            RealtimeEvent::AudioReceived {
                audio_bytes: 6400,
                chunk_bytes: 3200,
                message_count: 2
            }
        );
        assert_eq!(
            RealtimeEvent::parse(r#"{"type":"pong"}"#).unwrap(),
            RealtimeEvent::Pong
        );
        let e = RealtimeEvent::parse(
            r#"{"type":"session.failed","session_id":"rt_y","status":"failed","error":{"code":"provider_error","message":"boom","retryable":true},"result":{"text":"partial"}}"#,
        )
        .unwrap();
        let RealtimeEvent::SessionFailed {
            session_id,
            error,
            result,
        } = e
        else {
            panic!()
        };
        assert_eq!(
            (session_id.as_str(), error.message.as_str(), error.retryable),
            ("rt_y", "boom", true)
        );
        assert_eq!(result.unwrap().text, "partial");
        let e =
            RealtimeEvent::parse(r#"{"type":"session.failed","session_id":"rt_z","result":null}"#)
                .unwrap();
        assert!(matches!(
            e,
            RealtimeEvent::SessionFailed { result: None, .. }
        ));
        let e = RealtimeEvent::parse(r#"{"type":"brand.new","x":1}"#).unwrap();
        assert!(
            matches!(e, RealtimeEvent::Other { kind, raw } if kind == "brand.new" && raw["x"] == 1)
        );
        let e = RealtimeEvent::parse(r#"{"type":"transcript.committed","text":"a","words":[{"startSeconds":1,"endSeconds":2,"text":"a","speaker":0}]}"#).unwrap();
        let RealtimeEvent::Committed(c) = e else {
            panic!()
        };
        assert_eq!(c.words[0].speaker.as_deref(), Some("speaker_0"));
        assert!(RealtimeEvent::parse("not json").is_err());
    }

    #[test]
    fn upgrade_http_errors_keep_status_and_body() {
        let response = tungstenite::http::Response::builder()
            .status(400)
            .body(Some(
                br#"{"error":"advanced Mistral options require Voxtral 3 realtime"}"#.to_vec(),
            ))
            .unwrap();
        let err = ws_error(tungstenite::Error::Http(Box::new(response)));
        assert!(
            matches!(err, Error::BadRequest { status: 400, ref body } if body.contains("Voxtral 3")),
            "{err:?}"
        );
        let response = tungstenite::http::Response::builder()
            .status(401)
            .body(None)
            .unwrap();
        assert!(matches!(
            ws_error(tungstenite::Error::Http(Box::new(response))),
            Error::Unauthorized
        ));
    }
}
