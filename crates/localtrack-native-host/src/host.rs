//! Message handling: validated browser messages become observations, and the
//! Chrome popup can read status and drive the clock.

use std::sync::Arc;

use localtrack_collector_common::pipeline::IngestPipeline;
use localtrack_core::activity::{
    BrowserInteractionObservation, BrowserObservation, InteractionType, Observation,
};
use localtrack_core::time::now_ms;
use localtrack_storage::{repo, Database};
use serde_json::json;

use crate::protocol::{
    self, Ack, BrowserActivityPayload, BrowserInteractionPayload, Envelope, ErrorMessage,
    ProtocolError,
};

pub struct Host {
    pipeline: IngestPipeline,
    db: Arc<Database>,
    /// The extension origin Chrome started us for, when known.
    origin: Option<String>,
}

impl Host {
    pub fn new(db: Arc<Database>, origin: Option<String>) -> localtrack_storage::Result<Self> {
        // The desktop collector is a separate process; from the native host's
        // point of view the foreground window is unknown, so browser
        // observations are accepted on their own merit here.
        let pipeline = IngestPipeline::new(db.clone(), false)?;
        Ok(Self {
            pipeline,
            db,
            origin,
        })
    }

    pub fn pipeline_mut(&mut self) -> &mut IngestPipeline {
        &mut self.pipeline
    }

    /// Handle one raw message, returning the JSON response to send back.
    pub fn handle_raw(&mut self, raw: &[u8]) -> Vec<u8> {
        let now = now_ms();
        let envelope = match protocol::parse(raw, now) {
            Ok(envelope) => envelope,
            Err((message_id, err)) => {
                tracing::warn!(code = err.code(), "rejected native message");
                return serialize(&ErrorMessage::new(&message_id, err.code(), err.to_string()));
            }
        };

        match self.dispatch(&envelope, now) {
            Ok(payload) => serialize(&Ack::new(&envelope.message_id, now, payload)),
            Err(err) => {
                tracing::warn!(code = err.code(), "native message failed");
                // Never return internal details or stack traces (spec §36).
                serialize(&ErrorMessage::new(
                    &envelope.message_id,
                    err.code(),
                    err.to_string(),
                ))
            }
        }
    }

    fn dispatch(
        &mut self,
        envelope: &Envelope,
        now: i64,
    ) -> Result<Option<serde_json::Value>, ProtocolError> {
        match envelope.message_type.as_str() {
            protocol::TYPE_HELLO => {
                self.pipeline.reload().ok();
                // Record who connected so Diagnostics can show the versions in
                // play (spec §154). No activity data is involved.
                let origin = self.origin.clone();
                let _ = self.db.write(|tx| {
                    repo::settings::set(
                        tx,
                        "native_host_version",
                        &json!(env!("CARGO_PKG_VERSION")),
                        now,
                    )?;
                    repo::settings::set(tx, "chrome_extension_origin", &json!(origin), now)?;
                    repo::settings::set(tx, "chrome_connected_at_ms", &json!(now), now)
                });
                let settings = self.pipeline.settings();
                Ok(Some(json!({
                    "protocolVersion": protocol::PROTOCOL_VERSION,
                    "hostVersion": env!("CARGO_PKG_VERSION"),
                    "origin": self.origin,
                    // The extension sanitizes before queueing, so it needs to
                    // know the active privacy policy (spec §102).
                    "settings": {
                        "urlPolicy": settings.url_policy.as_str(),
                        "trackIncognito": settings.track_incognito,
                        "detailedInteractions": settings.detailed_interactions,
                        "trackingPaused": settings.tracking_paused,
                    },
                })))
            }

            protocol::TYPE_BROWSER_ACTIVITY => {
                // The desktop application may have clocked in or out since the
                // last message; the clock lives in the shared database.
                let _ = self.pipeline.refresh_clock_if_stale(3_000);
                let payload: BrowserActivityPayload =
                    serde_json::from_value(envelope.payload.clone())
                        .map_err(|_| ProtocolError::InvalidMessage)?;
                let payload = payload.validated(now)?;

                let observation = Observation::BrowserChanged(BrowserObservation {
                    captured_at_ms: payload.captured_at,
                    event: payload.event.clone(),
                    browser: payload.browser.clone(),
                    window_id: payload.window_id,
                    tab_id: payload.tab_id,
                    url: payload.url.clone(),
                    title: payload.title.clone(),
                    incognito: payload.incognito,
                    audible: payload.audible,
                    focused: payload.focused,
                });
                self.pipeline
                    .handle(observation)
                    .map_err(|_| ProtocolError::InvalidMessage)?;
                Ok(None)
            }

            protocol::TYPE_BROWSER_HEARTBEAT => {
                let _ = self.pipeline.refresh_clock_if_stale(3_000);
                let captured_at = envelope
                    .payload
                    .get("capturedAt")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(envelope.sent_at);
                self.pipeline
                    .handle(Observation::Heartbeat {
                        captured_at_ms: captured_at,
                        stream: "browser".into(),
                    })
                    .map_err(|_| ProtocolError::InvalidMessage)?;
                self.pipeline
                    .tick(now)
                    .map_err(|_| ProtocolError::InvalidMessage)?;
                Ok(None)
            }

            protocol::TYPE_BROWSER_INTERACTION => {
                let payload: BrowserInteractionPayload =
                    serde_json::from_value(envelope.payload.clone())
                        .map_err(|_| ProtocolError::InvalidMessage)?;
                let payload = payload.validated(now)?;
                let interaction = InteractionType::parse(&payload.interaction)
                    .ok_or(ProtocolError::InvalidMessage)?;
                self.pipeline
                    .handle(Observation::Interaction(BrowserInteractionObservation {
                        captured_at_ms: payload.captured_at,
                        interaction,
                        element: payload.element,
                        label: payload.label,
                        url: payload.url.clone(),
                        domain: payload
                            .url
                            .as_deref()
                            .and_then(localtrack_core::privacy::domain_of),
                    }))
                    .map_err(|_| ProtocolError::InvalidMessage)?;
                Ok(None)
            }

            protocol::TYPE_CLOCK_COMMAND => {
                let command = envelope
                    .payload
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or(ProtocolError::InvalidMessage)?;
                let at_ms = now;
                let result = match command {
                    "clock_in" => self.pipeline.clock_in(at_ms),
                    "clock_out" => self.pipeline.clock_out(at_ms),
                    "start_break" => self.pipeline.start_break(at_ms),
                    "end_break" => self.pipeline.end_break(at_ms),
                    _ => return Err(ProtocolError::InvalidMessage),
                };
                match result {
                    Ok(_) => Ok(Some(self.status_payload(now))),
                    // A rejected transition is an expected outcome, not a crash.
                    Err(_) => Err(ProtocolError::InvalidMessage),
                }
            }

            protocol::TYPE_STATUS_REQUEST => Ok(Some(self.status_payload(now))),

            _ => Err(ProtocolError::UnknownType),
        }
    }

    /// Status shown in the Chrome popup (spec §100).
    fn status_payload(&mut self, now: i64) -> serde_json::Value {
        self.pipeline.reload().ok();
        let clock = self.pipeline.clock().clone();
        let session_ms = clock
            .session
            .as_ref()
            .map(|s| s.clocked_duration_ms(now))
            .unwrap_or(0);

        let today_start = localtrack_core::time::local_day_start_ms(now);
        let active_ms = self.today_active_ms(today_start, now).unwrap_or(0);

        json!({
            "state": clock.state.as_str(),
            "sessionStartedAtMs": clock.session.as_ref().map(|s| s.started_at_ms),
            "sessionDurationMs": session_ms,
            "onBreakSinceMs": clock.open_break.as_ref().map(|b| b.started_at_ms),
            "trackingDecision": self.pipeline.tracking_decision(),
            "todayActiveMs": active_ms,
            "hostVersion": env!("CARGO_PKG_VERSION"),
        })
    }

    fn today_active_ms(&self, from_ms: i64, to_ms: i64) -> localtrack_storage::Result<i64> {
        use localtrack_core::aggregation::{summary::compute_summary, AggregationInput};
        use localtrack_core::interval::Interval;

        let (segments, sessions, breaks) = self.db.read(|conn| {
            Ok((
                repo::segments::load_range(conn, from_ms, to_ms)?,
                repo::sessions::list_sessions(conn, from_ms, to_ms)?,
                repo::sessions::breaks_in_range(conn, from_ms, to_ms)?,
            ))
        })?;
        let mut input = AggregationInput::new(Interval::new(from_ms, to_ms), to_ms);
        input.segments = segments;
        input.sessions = sessions;
        input.breaks = breaks;
        Ok(compute_summary(&input).active_ms)
    }
}

fn serialize<T: serde::Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|_| b"{\"version\":1,\"type\":\"error\"}".to_vec())
}
