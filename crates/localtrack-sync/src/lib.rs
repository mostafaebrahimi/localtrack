//! The only outbound network code in LocalTrack.
//!
//! It exists solely for employee mode: a device that is not enrolled never
//! constructs this transport, and a build without it has no HTTP stack at all.
//! Everything it sends is built and reviewed in `localtrack-core::managed`.

use std::time::Duration;

use localtrack_app::error::{AppError, Result};
use localtrack_app::managed::{EnrollRequest, EnrollResponse, SessionSyncResult, SyncTransport};
use localtrack_core::managed::{Enrollment, Heartbeat, ManagedPolicy, SessionSyncItem};
use serde::Serialize;

/// Paths on the organization's server.
pub mod endpoints {
    pub const ENROLL: &str = "/v1/enroll";
    pub const POLICY: &str = "/v1/policy";
    pub const REPORTS: &str = "/v1/reports/daily";
    pub const HEARTBEAT: &str = "/v1/heartbeat";
    pub const SESSIONS: &str = "/v1/sessions/sync";
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub struct HttpTransport {
    client: reqwest::blocking::Client,
}

impl HttpTransport {
    pub fn new() -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            // A tracking agent has no business following redirects to another
            // host. Cookies are off by default and the feature is not enabled.
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("LocalTrack/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|err| {
                AppError::Invalid(format!("could not start the network client: {err}"))
            })?;
        Ok(Self { client })
    }

    fn url(base: &str, path: &str) -> String {
        format!("{}{}", base.trim_end_matches('/'), path)
    }

    fn authorized(
        &self,
        method: reqwest::Method,
        enrollment: &Enrollment,
        path: &str,
    ) -> reqwest::blocking::RequestBuilder {
        self.client
            .request(method, Self::url(&enrollment.server_url, path))
            .bearer_auth(&enrollment.device_token)
            .header("X-LocalTrack-Device", &enrollment.device_id)
    }
}

/// Turn any transport failure into a message a person can act on.
fn describe(err: reqwest::Error) -> AppError {
    if err.is_timeout() {
        AppError::Invalid("The server did not answer in time.".into())
    } else if err.is_connect() {
        AppError::Invalid("The server could not be reached.".into())
    } else if err.is_decode() {
        AppError::Invalid("The server sent a response LocalTrack did not understand.".into())
    } else {
        AppError::Invalid("The request to the server failed.".into())
    }
}

fn check_status(response: reqwest::blocking::Response) -> Result<reqwest::blocking::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    // Never surface a server's raw body: it is untrusted text.
    let message = match status.as_u16() {
        401 | 403 => "This device is not authorized. Ask your administrator to re-enroll it.",
        404 => "The server does not offer this endpoint.",
        409 => "The server rejected the data as conflicting.",
        429 => "The server asked LocalTrack to slow down.",
        500..=599 => "The server reported an error.",
        _ => "The server rejected the request.",
    };
    Err(AppError::Invalid(format!("{message} (HTTP {status})")))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSyncRequest<'a> {
    device_id: &'a str,
    /// Server cursor from the previous exchange.
    since: Option<String>,
    sessions: &'a [SessionSyncItem],
}

impl SyncTransport for HttpTransport {
    fn enroll(&self, server_url: &str, request: &EnrollRequest) -> Result<EnrollResponse> {
        let response = self
            .client
            .post(Self::url(server_url, endpoints::ENROLL))
            .json(request)
            .send()
            .map_err(describe)?;
        let response = check_status(response)?;
        response.json::<EnrollResponse>().map_err(describe)
    }

    fn fetch_policy(&self, enrollment: &Enrollment) -> Result<Option<ManagedPolicy>> {
        let response = self
            .authorized(reqwest::Method::GET, enrollment, endpoints::POLICY)
            .send()
            .map_err(describe)?;

        // Nothing newer than what the device already has.
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        let response = check_status(response)?;
        let policy = response.json::<ManagedPolicy>().map_err(describe)?;
        Ok(Some(policy))
    }

    fn send_report(&self, enrollment: &Enrollment, payload_json: &str) -> Result<()> {
        let response = self
            .authorized(reqwest::Method::POST, enrollment, endpoints::REPORTS)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(payload_json.to_string())
            .send()
            .map_err(describe)?;
        check_status(response).map(|_| ())
    }

    fn send_heartbeat(&self, enrollment: &Enrollment, heartbeat: &Heartbeat) -> Result<()> {
        let response = self
            .authorized(reqwest::Method::POST, enrollment, endpoints::HEARTBEAT)
            .json(heartbeat)
            .send()
            .map_err(describe)?;
        check_status(response).map(|_| ())
    }

    fn sync_sessions(
        &self,
        enrollment: &Enrollment,
        since: Option<&str>,
        local_changes: &[SessionSyncItem],
    ) -> Result<SessionSyncResult> {
        let body = SessionSyncRequest {
            device_id: &enrollment.device_id,
            since: since.map(|s| s.to_string()),
            sessions: local_changes,
        };
        let response = self
            .authorized(reqwest::Method::POST, enrollment, endpoints::SESSIONS)
            .json(&body)
            .send()
            .map_err(describe)?;
        let response = check_status(response)?;
        response.json::<SessionSyncResult>().map_err(describe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_join_without_doubling_slashes() {
        assert_eq!(
            HttpTransport::url("https://track.example.com/", endpoints::REPORTS),
            "https://track.example.com/v1/reports/daily"
        );
        assert_eq!(
            HttpTransport::url("https://track.example.com", endpoints::POLICY),
            "https://track.example.com/v1/policy"
        );
    }

    #[test]
    fn a_transport_can_be_constructed() {
        assert!(HttpTransport::new().is_ok());
    }
}
