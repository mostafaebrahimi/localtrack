//! Native messaging protocol v1 (spec §34–§36, §106, §150).
//!
//! Every message is validated before it can influence anything: protocol
//! version, type, size, payload shape, timestamps, URLs, string lengths and
//! enum values. Errors never carry stack traces back to the extension.

use localtrack_core::limits;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = localtrack_core::NATIVE_PROTOCOL_VERSION;

/// Maximum accepted message size. Chrome itself caps messages at 1 MB.
pub const MAX_MESSAGE_BYTES: usize = limits::NATIVE_MESSAGE_MAX_BYTES;

/// How far a message timestamp may deviate from the host clock.
pub const MAX_CLOCK_SKEW_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub version: u32,
    pub message_id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub sent_at: i64,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ack {
    pub version: u32,
    pub message_id: String,
    #[serde(rename = "type")]
    pub message_type: &'static str,
    pub received_at: i64,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

impl Ack {
    pub fn new(message_id: &str, received_at: i64, payload: Option<serde_json::Value>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            message_id: message_id.to_string(),
            message_type: "ack",
            received_at,
            success: true,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorMessage {
    pub version: u32,
    pub message_id: String,
    #[serde(rename = "type")]
    pub message_type: &'static str,
    pub error: ErrorBody,
}

impl ErrorMessage {
    pub fn new(message_id: &str, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            message_id: message_id.to_string(),
            message_type: "error",
            error: ErrorBody {
                code,
                message: message.into(),
            },
        }
    }
}

/// Validation failures, mapped to stable machine-readable codes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("unsupported protocol version")]
    UnsupportedVersion,
    #[error("message is not valid JSON")]
    MalformedJson,
    #[error("message exceeds the maximum accepted size")]
    TooLarge,
    #[error("unsupported message schema")]
    InvalidMessage,
    #[error("unknown message type")]
    UnknownType,
    #[error("timestamp is implausible")]
    InvalidTimestamp,
}

impl ProtocolError {
    pub fn code(&self) -> &'static str {
        match self {
            ProtocolError::UnsupportedVersion => "UNSUPPORTED_PROTOCOL_VERSION",
            ProtocolError::MalformedJson => "MALFORMED_JSON",
            ProtocolError::TooLarge => "MESSAGE_TOO_LARGE",
            ProtocolError::InvalidMessage => "INVALID_MESSAGE",
            ProtocolError::UnknownType => "UNKNOWN_MESSAGE_TYPE",
            ProtocolError::InvalidTimestamp => "INVALID_TIMESTAMP",
        }
    }
}

/// Message types this host understands.
pub const TYPE_HELLO: &str = "hello";
pub const TYPE_BROWSER_ACTIVITY: &str = "browser.activity";
pub const TYPE_BROWSER_INTERACTION: &str = "browser.interaction";
pub const TYPE_BROWSER_HEARTBEAT: &str = "browser.heartbeat";
pub const TYPE_CLOCK_COMMAND: &str = "clock.command";
pub const TYPE_STATUS_REQUEST: &str = "status.request";

const KNOWN_TYPES: &[&str] = &[
    TYPE_HELLO,
    TYPE_BROWSER_ACTIVITY,
    TYPE_BROWSER_INTERACTION,
    TYPE_BROWSER_HEARTBEAT,
    TYPE_CLOCK_COMMAND,
    TYPE_STATUS_REQUEST,
];

/// Parse and validate a raw message body.
pub fn parse(raw: &[u8], now_ms: i64) -> Result<Envelope, (String, ProtocolError)> {
    if raw.len() > MAX_MESSAGE_BYTES {
        return Err((String::new(), ProtocolError::TooLarge));
    }
    let value: serde_json::Value =
        serde_json::from_slice(raw).map_err(|_| (String::new(), ProtocolError::MalformedJson))?;

    // The message id is recovered first so errors can be correlated.
    let message_id = value
        .get("messageId")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if version != PROTOCOL_VERSION {
        return Err((message_id, ProtocolError::UnsupportedVersion));
    }

    let envelope: Envelope = serde_json::from_value(value)
        .map_err(|_| (message_id.clone(), ProtocolError::InvalidMessage))?;

    if envelope.message_id.trim().is_empty() || envelope.message_id.len() > 128 {
        return Err((message_id, ProtocolError::InvalidMessage));
    }
    if !KNOWN_TYPES.contains(&envelope.message_type.as_str()) {
        return Err((envelope.message_id, ProtocolError::UnknownType));
    }
    if envelope.sent_at <= 0 || (envelope.sent_at - now_ms).abs() > MAX_CLOCK_SKEW_MS {
        return Err((envelope.message_id, ProtocolError::InvalidTimestamp));
    }

    Ok(envelope)
}

/// A validated `browser.activity` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserActivityPayload {
    pub event: String,
    pub captured_at: i64,
    pub browser: String,
    #[serde(default)]
    pub window_id: Option<i64>,
    #[serde(default)]
    pub tab_id: Option<i64>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub incognito: bool,
    #[serde(default)]
    pub audible: bool,
    #[serde(default = "default_true")]
    pub focused: bool,
}

fn default_true() -> bool {
    true
}

const KNOWN_EVENTS: &[&str] = &[
    "activated",
    "updated",
    "navigated",
    "title_changed",
    "focused",
    "blurred",
    "closed",
    "heartbeat",
];

impl BrowserActivityPayload {
    /// Enforce enum values and string length limits (spec §106).
    pub fn validated(mut self, now_ms: i64) -> Result<Self, ProtocolError> {
        if !KNOWN_EVENTS.contains(&self.event.as_str()) {
            return Err(ProtocolError::InvalidMessage);
        }
        if self.browser.len() > 64 {
            return Err(ProtocolError::InvalidMessage);
        }
        if self.captured_at <= 0 || (self.captured_at - now_ms).abs() > MAX_CLOCK_SKEW_MS {
            return Err(ProtocolError::InvalidTimestamp);
        }
        if let Some(url) = &self.url {
            if url.len() > limits::URL_MAX {
                return Err(ProtocolError::InvalidMessage);
            }
        }
        self.title = self
            .title
            .map(|t| limits::truncate(&limits::normalize_whitespace(&t), limits::PAGE_TITLE_MAX));
        Ok(self)
    }
}

/// A validated `browser.interaction` payload (spec §48–§51).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInteractionPayload {
    pub interaction: String,
    pub captured_at: i64,
    #[serde(default)]
    pub element: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

impl BrowserInteractionPayload {
    pub fn validated(mut self, now_ms: i64) -> Result<Self, ProtocolError> {
        if localtrack_core::activity::InteractionType::parse(&self.interaction).is_none() {
            return Err(ProtocolError::InvalidMessage);
        }
        if self.captured_at <= 0 || (self.captured_at - now_ms).abs() > MAX_CLOCK_SKEW_MS {
            return Err(ProtocolError::InvalidTimestamp);
        }
        // Element kind is a small allow-list; anything else is dropped rather
        // than stored, so page content can never leak in through this field.
        self.element = self.element.filter(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "button" | "a" | "link" | "form" | "input" | "select" | "summary" | "div"
            )
        });
        self.label = self.label.map(|l| {
            limits::truncate(
                &limits::normalize_whitespace(&l),
                limits::INTERACTION_LABEL_MAX,
            )
        });
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_787_253_458_123;

    fn message(json: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&json).unwrap()
    }

    #[test]
    fn accepts_a_well_formed_message() {
        let raw = message(serde_json::json!({
            "version": 1,
            "messageId": "abc",
            "type": "browser.activity",
            "sentAt": NOW,
            "payload": {"event": "activated", "capturedAt": NOW, "browser": "chrome"}
        }));
        let envelope = parse(&raw, NOW).unwrap();
        assert_eq!(envelope.message_type, "browser.activity");
    }

    #[test]
    fn rejects_unsupported_protocol_versions() {
        let raw = message(serde_json::json!({
            "version": 2, "messageId": "abc", "type": "hello", "sentAt": NOW
        }));
        let (_, err) = parse(&raw, NOW).unwrap_err();
        assert_eq!(err, ProtocolError::UnsupportedVersion);
        assert_eq!(err.code(), "UNSUPPORTED_PROTOCOL_VERSION");
    }

    #[test]
    fn rejects_unknown_types_and_bad_json() {
        let raw = message(serde_json::json!({
            "version": 1, "messageId": "abc", "type": "shell.exec", "sentAt": NOW
        }));
        assert_eq!(parse(&raw, NOW).unwrap_err().1, ProtocolError::UnknownType);
        assert_eq!(
            parse(b"{not json", NOW).unwrap_err().1,
            ProtocolError::MalformedJson
        );
    }

    #[test]
    fn rejects_implausible_timestamps() {
        let raw = message(serde_json::json!({
            "version": 1, "messageId": "abc", "type": "hello", "sentAt": 1
        }));
        assert_eq!(
            parse(&raw, NOW).unwrap_err().1,
            ProtocolError::InvalidTimestamp
        );
    }

    #[test]
    fn rejects_oversized_messages() {
        let raw = vec![b'x'; MAX_MESSAGE_BYTES + 1];
        assert_eq!(parse(&raw, NOW).unwrap_err().1, ProtocolError::TooLarge);
    }

    #[test]
    fn activity_payload_validation_enforces_enums_and_limits() {
        let payload = BrowserActivityPayload {
            event: "activated".into(),
            captured_at: NOW,
            browser: "chrome".into(),
            window_id: None,
            tab_id: None,
            url: Some("https://example.com".into()),
            title: Some("   Lots   of   space  ".into()),
            incognito: false,
            audible: false,
            focused: true,
        };
        let validated = payload.clone().validated(NOW).unwrap();
        assert_eq!(validated.title.as_deref(), Some("Lots of space"));

        let mut bad = payload.clone();
        bad.event = "keylog".into();
        assert_eq!(
            bad.validated(NOW).unwrap_err(),
            ProtocolError::InvalidMessage
        );

        let mut oversized = payload;
        oversized.url = Some("x".repeat(limits::URL_MAX + 1));
        assert_eq!(
            oversized.validated(NOW).unwrap_err(),
            ProtocolError::InvalidMessage
        );
    }

    #[test]
    fn interaction_payload_drops_unexpected_elements_and_long_labels() {
        let payload = BrowserInteractionPayload {
            interaction: "button_click".into(),
            captured_at: NOW,
            element: Some("script".into()),
            label: Some("x".repeat(500)),
            url: None,
        };
        let validated = payload.validated(NOW).unwrap();
        assert!(validated.element.is_none());
        assert_eq!(
            validated.label.unwrap().chars().count(),
            limits::INTERACTION_LABEL_MAX
        );
    }
}
