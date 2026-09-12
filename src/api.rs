//! TypeWhisper recorder HTTP client.

use crate::discovery::Discovery;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

pub trait Api: Send + Sync {
    fn start(&mut self) -> Result<RecorderSession, ApiError>;
    fn stop(&mut self) -> Result<RecorderSession, ApiError>;
    fn session(&mut self, id: &str) -> Result<RecorderSession, ApiError>;
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RecorderSession {
    pub id: String,
    pub status: SessionStatus,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub output_file: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Recording,
    Finalizing,
    Completed,
    Failed,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("TypeWhisper API unavailable: {0}")]
    Unavailable(String),
    #[error("TypeWhisper API token rejected (401)")]
    Unauthorized,
    #[error("TypeWhisper HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("invalid TypeWhisper response: {0}")]
    InvalidResponse(String),
}

pub struct HttpApi {
    agent: ureq::Agent,
    base: String,
    token: Option<String>,
    discovery_path: Option<PathBuf>,
}

impl HttpApi {
    pub fn new(port: u16, token: Option<String>) -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(1))
                .timeout(Duration::from_secs(5))
                .build(),
            base: format!("http://127.0.0.1:{port}"),
            token,
            discovery_path: None,
        }
    }

    pub fn from_discovery(discovery: &Discovery, path: PathBuf) -> Self {
        let mut api = Self::new(discovery.port, discovery.token.clone());
        api.discovery_path = Some(path);
        api
    }

    fn call(&mut self, method: &str, path: &str) -> Result<RecorderSession, ApiError> {
        match self.attempt(method, path) {
            Err(ApiError::Unauthorized) if self.discovery_path.is_some() => {
                log::warn!("TypeWhisper rejected API token; reloading discovery and retrying once");
                self.reload_discovery()?;
                self.attempt(method, path)
            }
            result => result,
        }
    }

    fn attempt(&self, method: &str, path: &str) -> Result<RecorderSession, ApiError> {
        let url = format!("{}{}", self.base, path);
        if method == "POST" {
            log::info!("Sending API request: {method} {url}");
        } else {
            log::debug!("Sending API request: {method} {url}");
        }
        let mut request = match method {
            "GET" => self.agent.get(&url),
            "POST" => self.agent.post(&url),
            other => unreachable!("unsupported HTTP method {other}"),
        };
        if let Some(token) = &self.token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        match request.call() {
            Ok(response) => response
                .into_json::<RecorderSession>()
                .map_err(|e| ApiError::InvalidResponse(e.to_string())),
            Err(ureq::Error::Status(401, _)) => Err(ApiError::Unauthorized),
            Err(ureq::Error::Status(status, response)) => {
                let fallback = response.status_text().to_string();
                let message = error_message(response).unwrap_or(fallback);
                Err(ApiError::Http { status, message })
            }
            Err(ureq::Error::Transport(transport)) => {
                Err(ApiError::Unavailable(transport.to_string()))
            }
        }
    }

    fn reload_discovery(&mut self) -> Result<(), ApiError> {
        let path = self
            .discovery_path
            .as_ref()
            .expect("reload_discovery called without a discovery path");
        match crate::discovery::load(path) {
            Ok(discovery) => {
                self.base = format!("http://127.0.0.1:{}", discovery.port);
                self.token = discovery.token;
                log::info!("TypeWhisper API rediscovered: {}", self.base);
                Ok(())
            }
            Err(e) => Err(ApiError::Unavailable(e.to_string())),
        }
    }
}

impl Api for HttpApi {
    fn start(&mut self) -> Result<RecorderSession, ApiError> {
        self.call("POST", "/v1/recorder/start?mic=true&system_audio=false")
    }

    fn stop(&mut self) -> Result<RecorderSession, ApiError> {
        self.call("POST", "/v1/recorder/stop")
    }

    fn session(&mut self, id: &str) -> Result<RecorderSession, ApiError> {
        self.call("GET", &format!("/v1/recorder/session?id={id}"))
    }
}

fn error_message(response: ureq::Response) -> Option<String> {
    let body = response.into_string().ok()?;
    let value: serde_json::Value = serde_json::from_str(&body).ok()?;
    value
        .get("error")?
        .get("message")?
        .as_str()
        .map(str::to_owned)
}

/// Probes the always-public `GET /v1/status` endpoint from the discovery file.
/// Used by `--check`; absence of TypeWhisper is reported as an error here and
/// treated as a warning by the caller.
pub fn probe_status(discovery: &Discovery) -> Result<(), ApiError> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(1))
        .timeout(Duration::from_secs(5))
        .build();
    let url = format!("http://127.0.0.1:{}/v1/status", discovery.port);
    match agent.get(&url).call() {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(401, _)) => Err(ApiError::Unauthorized),
        Err(ureq::Error::Status(status, response)) => {
            let fallback = response.status_text().to_string();
            let message = error_message(response).unwrap_or(fallback);
            Err(ApiError::Http { status, message })
        }
        Err(ureq::Error::Transport(transport)) => Err(ApiError::Unavailable(transport.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn session_body(id: &str, status: &str) -> String {
        format!(r#"{{"id":"{id}","status":"{status}"}}"#)
    }

    #[test]
    fn start_parses_session() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/recorder/start")
                .query_param("mic", "true")
                .query_param("system_audio", "false");
            then.status(200).body(session_body("a", "recording"));
        });

        let mut api = HttpApi::new(server.port(), None);
        let session = api.start().unwrap();

        assert_eq!(session.id, "a");
        assert_eq!(session.status, SessionStatus::Recording);
        assert_eq!(session.text, None);
        assert_eq!(session.output_file, None);
        assert_eq!(session.error, None);
        mock.assert_hits(1);
    }

    #[test]
    fn stop_parses_finalizing() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/recorder/stop");
            then.status(200).body(session_body("a", "finalizing"));
        });

        let mut api = HttpApi::new(server.port(), None);
        let session = api.stop().unwrap();

        assert_eq!(session.id, "a");
        assert_eq!(session.status, SessionStatus::Finalizing);
        mock.assert_hits(1);
    }

    #[test]
    fn session_parses_completed_text() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/v1/recorder/session")
                .query_param("id", "a");
            then.status(200)
                .body(r#"{"id":"a","status":"completed","text":"hi","output_file":"/tmp/r.wav"}"#);
        });

        let mut api = HttpApi::new(server.port(), None);
        let session = api.session("a").unwrap();

        assert_eq!(session.status, SessionStatus::Completed);
        assert_eq!(session.text.as_deref(), Some("hi"));
        assert_eq!(session.output_file.as_deref(), Some("/tmp/r.wav"));
        mock.assert_hits(1);
    }

    #[test]
    fn session_parses_failed() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/v1/recorder/session")
                .query_param("id", "a");
            then.status(200)
                .body(r#"{"id":"a","status":"failed","error":"finalTranscription: boom"}"#);
        });

        let mut api = HttpApi::new(server.port(), None);
        let session = api.session("a").unwrap();

        assert_eq!(session.status, SessionStatus::Failed);
        assert_eq!(session.error.as_deref(), Some("finalTranscription: boom"));
        mock.assert_hits(1);
    }

    #[test]
    fn auth_header_sent_when_token_present() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/recorder/start")
                .header("Authorization", "Bearer tok");
            then.status(200).body(session_body("a", "recording"));
        });

        let mut api = HttpApi::new(server.port(), Some("tok".into()));
        api.start().unwrap();

        mock.assert_hits(1);
    }

    #[test]
    fn status_409_maps_to_http_error() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/recorder/start");
            then.status(409).json_body(
                serde_json::json!({"error":{"code":"bad_request","message":"Already recording"}}),
            );
        });

        let mut api = HttpApi::new(server.port(), None);
        let err = api.start().unwrap_err();

        match err {
            ApiError::Http { status, message } => {
                assert_eq!(status, 409);
                assert!(
                    message.contains("Already recording"),
                    "message was {message:?}"
                );
            }
            other => panic!("expected Http error, got {other:?}"),
        }
        mock.assert_hits(1);
    }

    #[test]
    fn status_401_maps_to_unauthorized() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/recorder/start");
            then.status(401)
                .body(r#"{"error":{"message":"invalid token"}}"#);
        });

        let mut api = HttpApi::new(server.port(), None);
        let err = api.start().unwrap_err();

        assert!(matches!(err, ApiError::Unauthorized), "got {err:?}");
        mock.assert_hits(1);
    }

    #[test]
    fn unauthorized_reloads_discovery_and_retries_once() {
        let server = MockServer::start();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api-discovery.json");
        std::fs::write(
            &path,
            format!(r#"{{"port":{},"token":"old"}}"#, server.port()),
        )
        .unwrap();
        let discovery = crate::discovery::load(&path).unwrap();

        let old_token = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/recorder/start")
                .header("Authorization", "Bearer old");
            then.status(401)
                .body(r#"{"error":{"message":"token expired"}}"#);
        });
        let new_token = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/recorder/start")
                .header("Authorization", "Bearer new");
            then.status(200).body(session_body("a", "recording"));
        });

        // TypeWhisper rotated its token after the discovery file was first read.
        std::fs::write(
            &path,
            format!(r#"{{"port":{},"token":"new"}}"#, server.port()),
        )
        .unwrap();

        let mut api = HttpApi::from_discovery(&discovery, path);
        let session = api.start().unwrap();

        assert_eq!(session.id, "a");
        old_token.assert_hits(1);
        new_token.assert_hits(1);
    }

    #[test]
    fn persistent_unauthorized_stops_after_one_retry() {
        let server = MockServer::start();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api-discovery.json");
        std::fs::write(
            &path,
            format!(r#"{{"port":{},"token":"old"}}"#, server.port()),
        )
        .unwrap();
        let discovery = crate::discovery::load(&path).unwrap();

        let first = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/recorder/start")
                .header("Authorization", "Bearer old");
            then.status(401)
                .body(r#"{"error":{"message":"token expired"}}"#);
        });
        let second = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/recorder/start")
                .header("Authorization", "Bearer new");
            then.status(401)
                .body(r#"{"error":{"message":"token expired"}}"#);
        });

        // The reloaded discovery still rejects the token.
        std::fs::write(
            &path,
            format!(r#"{{"port":{},"token":"new"}}"#, server.port()),
        )
        .unwrap();

        let mut api = HttpApi::from_discovery(&discovery, path);
        let err = api.start().unwrap_err();

        assert!(matches!(err, ApiError::Unauthorized), "got {err:?}");
        first.assert_hits(1);
        second.assert_hits(1);
    }

    #[test]
    fn invalid_json_response_is_invalid_response() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/recorder/start");
            then.status(200).body("not json");
        });

        let mut api = HttpApi::new(server.port(), None);
        let err = api.start().unwrap_err();

        assert!(matches!(err, ApiError::InvalidResponse(_)), "got {err:?}");
        mock.assert_hits(1);
    }

    #[test]
    fn connection_refused_is_unavailable() {
        let mut api = HttpApi::new(1, None);
        let err = api.start().unwrap_err();

        assert!(matches!(err, ApiError::Unavailable(_)), "got {err:?}");
    }

    fn discovery_for(port: u16) -> Discovery {
        Discovery {
            port,
            token: None,
            version: None,
        }
    }

    #[test]
    fn status_probe_succeeds_on_200() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v1/status");
            then.status(200).body(r#"{"version":1}"#);
        });

        probe_status(&discovery_for(server.port())).unwrap();

        mock.assert_hits(1);
    }

    #[test]
    fn status_probe_reports_http_errors() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v1/status");
            then.status(503)
                .body(r#"{"error":{"message":"starting up"}}"#);
        });

        match probe_status(&discovery_for(server.port())).unwrap_err() {
            ApiError::Http { status, message } => {
                assert_eq!(status, 503);
                assert!(message.contains("starting up"), "message was {message:?}");
            }
            other => panic!("expected Http error, got {other:?}"),
        }
        mock.assert_hits(1);
    }

    #[test]
    fn status_probe_connection_refused_is_unavailable() {
        assert!(matches!(
            probe_status(&discovery_for(1)),
            Err(ApiError::Unavailable(_))
        ));
    }
}
