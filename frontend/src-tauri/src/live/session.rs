//! One live session task: connect (buffering), stream, reconnect, finish.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{info, warn};
use peacockery_voice::{
    Channel, Error, LineBuilder, LineBuilderConfig, RealtimeEvent, RealtimeOptions, RealtimeSender, RealtimeSession,
    SpeakerLabels, FRAME_SAMPLES,
};
use tokio::sync::mpsc;

use super::taps::LiveFrame;
use super::{LiveState, Shared};
use crate::voice::describe_error;

/// Waits before reconnect attempts 1…5.
const BACKOFF_SECONDS: [u64; 5] = [1, 2, 5, 10, 10];
/// Audio kept while (re)connecting: 10 s of 100 ms frames, oldest dropped first.
const BACKLOG_FRAMES: usize = 100;
/// With no audio frame for this long (pause, idle sources) silence frames keep the session alive.
const KEEPALIVE_AFTER: Duration = Duration::from_millis(300);
const TICK: Duration = Duration::from_millis(100);
/// A session that stayed up this long resets the failure count.
const STABLE_SESSION: Duration = Duration::from_secs(30);
/// Time for `session.completed` after the close message at stop.
const FINISH_TIMEOUT: Duration = Duration::from_secs(18);
/// Time to collect a broken session's terminal event before reconnecting.
const ABANDON_TIMEOUT: Duration = Duration::from_secs(2);

static SILENCE: [i16; FRAME_SAMPLES] = [0; FRAME_SAMPLES];

/// Audio waiting for a connection.
#[derive(Default)]
struct Backlog {
    frames: VecDeque<LiveFrame>,
    dropped: u64,
}

impl Backlog {
    fn push(&mut self, frame: LiveFrame) {
        if self.frames.len() >= BACKLOG_FRAMES {
            self.frames.pop_front();
            self.dropped += 1;
        }
        self.frames.push_back(frame);
    }
}

/// Input side of a session task.
struct Input {
    rx: mpsc::Receiver<LiveFrame>,
    open: bool,
    backlog: Backlog,
    /// Recording time of the newest frame seen.
    last_end: f64,
}

impl Input {
    async fn recv(&mut self) -> Option<LiveFrame> {
        let frame = self.rx.recv().await;
        match &frame {
            Some(frame) => self.last_end = frame.end,
            None => self.open = false,
        }
        frame
    }

    /// Sleep `delay` while buffering. False when the input closed meanwhile.
    async fn wait(&mut self, delay: Duration) -> bool {
        let sleep = tokio::time::sleep(delay);
        tokio::pin!(sleep);
        while self.open {
            tokio::select! {
                _ = &mut sleep => return true,
                frame = self.recv() => if let Some(frame) = frame { self.backlog.push(frame) },
            }
        }
        false
    }
}

/// How a connected session ended.
enum Ended {
    /// Input closed and the session finished: done.
    Stopped,
    /// The session broke while recording: reconnect.
    Lost(Error),
}

fn retryable(error: &Error) -> bool {
    error.is_transient()
        || matches!(error, Error::RealtimeClosed(_) | Error::Decode(_))
        || matches!(error, Error::BadRequest { status: 408 | 429, .. })
}

pub(super) async fn run(shared: Arc<Shared>, channel: Channel, rx: mpsc::Receiver<LiveFrame>) {
    let mut input = Input { rx, open: true, backlog: Backlog::default(), last_end: 0.0 };
    let mut model = shared.config.model.clone();
    let mut failures = 0usize;
    let mut state = LiveState::Connecting;
    let mut message = None;
    loop {
        shared.status_model(channel, state, message.take(), &model);
        let connected = {
            let connect = connect(&shared, channel, &model);
            tokio::pin!(connect);
            loop {
                tokio::select! {
                    result = &mut connect => break result,
                    frame = input.recv(), if input.open => if let Some(frame) = frame { input.backlog.push(frame) },
                }
            }
        };
        let error = match connected {
            Ok((sender, events)) => {
                shared.status_model(channel, LiveState::Live, None, &model);
                let up = Instant::now();
                match stream(&shared, channel, &model, sender, events, &mut input).await {
                    Ended::Stopped => {
                        shared.status_model(channel, LiveState::Closed, None, &model);
                        return;
                    }
                    Ended::Lost(error) => {
                        if up.elapsed() >= STABLE_SESSION {
                            failures = 0;
                        }
                        error
                    }
                }
            }
            Err(error) => error,
        };
        failures += 1;
        let reason = describe_error(&error);
        if !input.open {
            shared.status_model(channel, LiveState::Closed, Some(reason), &model);
            return;
        }
        if should_fallback(&model, failures, &error) {
            model = "deepgram-nova-3".into();
            failures = 0;
            state = LiveState::Reconnecting;
            message = Some(format!("Scribe unavailable: {}. Switching to Nova; local audio recording continues.", reason));
            continue;
        }
        if !retryable(&error) || failures > BACKOFF_SECONDS.len() {
            warn!("Live {:?} session gave up after {} failures: {}", channel, failures, reason);
            shared.status_model(channel, LiveState::Failed, Some(reason), &model);
            // Keep draining so the pipeline never sees a full channel.
            while input.recv().await.is_some() {}
            return;
        }
        let delay = BACKOFF_SECONDS[failures - 1];
        warn!("Live {:?} session failed ({}); retry {} in {} s", channel, reason, failures, delay);
        shared.status_model(channel, LiveState::Reconnecting, Some(format!("{} — retrying in {} s", reason, delay)), &model);
        if !input.wait(Duration::from_secs(delay)).await {
            shared.status_model(channel, LiveState::Closed, None, &model);
            return;
        }
        state = LiveState::Reconnecting;
        message = Some(format!("Reconnecting (attempt {} of {})", failures + 1, BACKOFF_SECONDS.len() + 1));
    }
}

async fn connect(
    shared: &Shared,
    channel: Channel,
    model: &str,
) -> Result<(RealtimeSender, mpsc::Receiver<RealtimeEvent>), Error> {
    let config = &shared.config;
    let catalog = shared.client.models().await?;
    let route = catalog
        .realtime_model_id(model)
        .ok_or_else(|| Error::Config(format!("{} has no realtime route on this server", config.model)))?
        .to_string();
    let mut options = RealtimeOptions::new(route);
    options.language = config.language.clone();
    options.diarize = config.diarize_for_model(channel, model);
    options.keyterms = config.keyterms.clone();
    options.client_session_id = Some(config.meeting_id.clone());
    RealtimeSession::connect(&shared.client, options).await
}

fn builder_for(shared: &Shared, channel: Channel, model: &str, offset: f64) -> LineBuilder {
    let config = &shared.config;
    let speakers =
        SpeakerLabels::for_session(config.mode, channel, config.diarize_for_model(channel, model), &config.me_label, &config.them_label);
    let mut builder_config = LineBuilderConfig::new(channel, model, speakers);
    builder_config.session_offset = offset.max(0.0);
    LineBuilder::new(builder_config)
}

fn note_event(shared: &Shared, event: &RealtimeEvent) {
    match event {
        RealtimeEvent::SessionStarted { session_id, .. }
        | RealtimeEvent::SessionCompleted { session_id, .. }
        | RealtimeEvent::SessionFailed { session_id, .. } => shared.add_session_id(session_id),
        _ => {}
    }
}

/// Stream until the input closes (then finish cleanly) or the session breaks.
async fn stream(
    shared: &Shared,
    channel: Channel,
    model: &str,
    mut sender: RealtimeSender,
    mut events: mpsc::Receiver<RealtimeEvent>,
    input: &mut Input,
) -> Ended {
    let offset = input
        .backlog
        .frames
        .front()
        .map(|frame| frame.end - frame.duration())
        .unwrap_or(input.last_end);
    let mut builder = builder_for(shared, channel, model, offset);
    let mut sent = offset;
    if input.backlog.dropped > 0 {
        warn!("Live {:?}: {} frames of audio were dropped while connecting", channel, input.backlog.dropped);
        input.backlog.dropped = 0;
    }
    while let Some(frame) = input.backlog.frames.pop_front() {
        if let Err(error) = sender.send_pcm(&frame.samples).await {
            return lose(shared, sender, events, builder, sent, error).await;
        }
        sent = frame.end;
    }
    if !input.open {
        return finish(shared, sender, events, builder, sent).await;
    }

    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_frame = Instant::now();
    loop {
        tokio::select! {
            frame = input.recv() => match frame {
                Some(frame) => {
                    if let Err(error) = sender.send_pcm(&frame.samples).await {
                        input.backlog.push(frame);
                        return lose(shared, sender, events, builder, sent, error).await;
                    }
                    sent = frame.end;
                    last_frame = Instant::now();
                }
                None => return finish(shared, sender, events, builder, sent).await,
            },
            event = events.recv() => match event {
                Some(event) => {
                    note_event(shared, &event);
                    shared.apply(builder.on_event(&event, sent));
                    match event {
                        RealtimeEvent::SessionFailed { error, .. } => {
                            let error = Error::SessionFailed { code: error.code, message: error.message, retryable: error.retryable };
                            let _ = sender.finish(ABANDON_TIMEOUT).await;
                            return Ended::Lost(error);
                        }
                        RealtimeEvent::SessionCompleted { .. } => {
                            let _ = sender.finish(ABANDON_TIMEOUT).await;
                            return Ended::Lost(Error::RealtimeClosed("the server ended the session".into()));
                        }
                        _ => {}
                    }
                }
                None => {
                    shared.apply(builder.finish(sent));
                    let error = match sender.finish(ABANDON_TIMEOUT).await {
                        Ok(_) => Error::RealtimeClosed("the server ended the session".into()),
                        Err(error) => error,
                    };
                    return Ended::Lost(error);
                }
            },
            _ = tick.tick() => {
                shared.apply(builder.tick(sent));
                if last_frame.elapsed() >= KEEPALIVE_AFTER {
                    // Silence does not advance the recording clock (`sent`).
                    if let Err(error) = sender.send_pcm(&SILENCE).await {
                        return lose(shared, sender, events, builder, sent, error).await;
                    }
                }
            }
        }
    }
}

/// A send failed: keep what the provider already said, then report the failure.
async fn lose(
    shared: &Shared,
    sender: RealtimeSender,
    mut events: mpsc::Receiver<RealtimeEvent>,
    mut builder: LineBuilder,
    sent: f64,
    error: Error,
) -> Ended {
    while let Ok(event) = events.try_recv() {
        note_event(shared, &event);
        shared.apply(builder.on_event(&event, sent));
    }
    shared.apply(builder.finish(sent));
    let error = match sender.finish(ABANDON_TIMEOUT).await {
        Err(reason) => reason,
        Ok(_) => error,
    };
    Ended::Lost(error)
}

/// Close the session and keep pumping events until its terminal outcome.
async fn finish(
    shared: &Shared,
    sender: RealtimeSender,
    mut events: mpsc::Receiver<RealtimeEvent>,
    mut builder: LineBuilder,
    sent: f64,
) -> Ended {
    let closing = sender.finish(FINISH_TIMEOUT);
    tokio::pin!(closing);
    let mut events_open = true;
    let outcome = loop {
        tokio::select! {
            outcome = &mut closing => break outcome,
            event = events.recv(), if events_open => match event {
                Some(event) => {
                    note_event(shared, &event);
                    shared.apply(builder.on_event(&event, sent));
                }
                None => events_open = false,
            },
        }
    };
    while let Ok(event) = events.try_recv() {
        note_event(shared, &event);
        shared.apply(builder.on_event(&event, sent));
    }
    match &outcome {
        Ok(terminal) => {
            shared.add_session_id(terminal.session_id());
            info!("Live session {} finished", terminal.session_id());
        }
        Err(e) => warn!("Live session did not finish cleanly: {}", describe_error(e)),
    }
    shared.apply(builder.finish(sent));
    Ended::Stopped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backlog_keeps_the_newest_ten_seconds() {
        let mut backlog = Backlog::default();
        for i in 0..150 {
            backlog.push(LiveFrame { samples: vec![0; FRAME_SAMPLES], end: i as f64 * 0.1 });
        }
        assert_eq!(backlog.frames.len(), BACKLOG_FRAMES);
        assert_eq!(backlog.dropped, 50);
        assert!((backlog.frames.front().unwrap().end - 5.0).abs() < 1e-9);
    }

    #[test]
    fn scribe_falls_back_after_retry_or_provider_rejection() {
        let transient = Error::Network("unavailable".into());
        assert!(!should_fallback("elevenlabs-scribe_v3", 1, &transient));
        assert!(should_fallback("elevenlabs-scribe_v3", 2, &transient));
        assert!(should_fallback("elevenlabs-scribe_v3", 1, &Error::SessionFailed { code: "provider_error".into(), message: "unsupported model".into(), retryable: false }));
        assert!(!should_fallback("deepgram-nova-3", 2, &transient));
        assert!(!should_fallback("elevenlabs-scribe_v3", 2, &Error::Unauthorized));
        assert!(!should_fallback("elevenlabs-scribe_v3", 2, &Error::Cancelled));
    }

    #[test]
    fn retry_only_what_can_recover() {
        assert!(retryable(&Error::Network("x".into())));
        assert!(retryable(&Error::RealtimeClosed("x".into())));
        assert!(retryable(&Error::BadRequest { status: 429, body: String::new() }));
        assert!(retryable(&Error::SessionFailed { code: "x".into(), message: "429".into(), retryable: true }));
        assert!(!retryable(&Error::Unauthorized));
        assert!(!retryable(&Error::BadRequest { status: 400, body: String::new() }));
        assert!(!retryable(&Error::Config("x".into())));
    }
}

fn should_fallback(model: &str, failures: usize, error: &Error) -> bool {
    model == "elevenlabs-scribe_v3"
        && !matches!(error, Error::Unauthorized | Error::Forbidden(_) | Error::Cancelled)
        && (failures >= 2 || !retryable(error))
}
