//! Recording session state machine.

use crate::api::{Api, ApiError, SessionStatus};
use std::time::{Duration, Instant};

const MIN_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Idle,
    Recording { id: String, started: Instant },
    Finalizing { id: String, deadline: Instant },
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    None,
    Transcribed {
        text: Option<String>,
        output_file: Option<String>,
    },
    Error(String),
}

pub struct Engine<A: Api> {
    api: A,
    state: State,
    last_error: Option<String>,
}

impl<A: Api> Engine<A> {
    pub fn new(api: A) -> Self {
        Self {
            api,
            state: State::Idle,
            last_error: None,
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn started_at(&self) -> Option<Instant> {
        match &self.state {
            State::Recording { started, .. } => Some(*started),
            _ => None,
        }
    }

    pub fn toggle(&mut self, now: Instant) -> Update {
        match &self.state {
            State::Recording { started, .. } => {
                let started = *started;
                match self.api.stop() {
                    Ok(session) => {
                        let elapsed = now.checked_duration_since(started).unwrap_or_default();
                        let timeout = MIN_TIMEOUT.max(elapsed.saturating_mul(3));
                        self.state = State::Finalizing {
                            id: session.id,
                            deadline: now + timeout,
                        };
                        self.last_error = None;
                        Update::None
                    }
                    Err(e) => self.fail(e),
                }
            }
            State::Finalizing { .. } => Update::None,
            State::Idle | State::Error { .. } => match self.api.start() {
                Ok(session) if session.status == SessionStatus::Recording => {
                    self.state = State::Recording {
                        id: session.id,
                        started: now,
                    };
                    self.last_error = None;
                    Update::None
                }
                Ok(session) => {
                    let message = format!("unexpected start status: {:?}", session.status);
                    self.fail_message(message)
                }
                Err(e) => self.fail(e),
            },
        }
    }

    pub fn tick(&mut self, now: Instant) -> Update {
        let (id, deadline) = match &self.state {
            State::Finalizing { id, deadline } => (id.clone(), *deadline),
            _ => return Update::None,
        };
        if now >= deadline {
            return self.fail_message("timed out waiting for transcription".into());
        }
        match self.api.session(&id) {
            Ok(session) => match session.status {
                SessionStatus::Completed => {
                    self.state = State::Idle;
                    self.last_error = None;
                    Update::Transcribed {
                        text: session.text,
                        output_file: session.output_file,
                    }
                }
                SessionStatus::Failed => self.fail_message(
                    session
                        .error
                        .unwrap_or_else(|| "transcription failed".into()),
                ),
                SessionStatus::Recording | SessionStatus::Finalizing => Update::None,
            },
            Err(e) => self.fail(e),
        }
    }

    fn fail(&mut self, error: ApiError) -> Update {
        self.fail_message(error.to_string())
    }

    fn fail_message(&mut self, message: String) -> Update {
        self.state = State::Error {
            message: message.clone(),
        };
        self.last_error = Some(message.clone());
        Update::Error(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ApiError, RecorderSession, SessionStatus};
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct MockApi {
        start_results: VecDeque<Result<RecorderSession, ApiError>>,
        stop_results: VecDeque<Result<RecorderSession, ApiError>>,
        session_results: VecDeque<Result<RecorderSession, ApiError>>,
        start_calls: usize,
        stop_calls: usize,
        session_calls: usize,
    }

    impl MockApi {
        fn new() -> Self {
            Self::default()
        }

        fn start(mut self, result: Result<RecorderSession, ApiError>) -> Self {
            self.start_results.push_back(result);
            self
        }

        fn stop(mut self, result: Result<RecorderSession, ApiError>) -> Self {
            self.stop_results.push_back(result);
            self
        }

        fn session(mut self, result: Result<RecorderSession, ApiError>) -> Self {
            self.session_results.push_back(result);
            self
        }
    }

    impl Api for MockApi {
        fn start(&mut self) -> Result<RecorderSession, ApiError> {
            self.start_calls += 1;
            self.start_results
                .pop_front()
                .expect("unexpected start call")
        }

        fn stop(&mut self) -> Result<RecorderSession, ApiError> {
            self.stop_calls += 1;
            self.stop_results.pop_front().expect("unexpected stop call")
        }

        fn session(&mut self, _id: &str) -> Result<RecorderSession, ApiError> {
            self.session_calls += 1;
            self.session_results
                .pop_front()
                .expect("unexpected session call")
        }
    }

    fn recording(id: &str) -> RecorderSession {
        RecorderSession {
            id: id.into(),
            status: SessionStatus::Recording,
            text: None,
            output_file: None,
            error: None,
        }
    }

    fn finalizing(id: &str) -> RecorderSession {
        RecorderSession {
            id: id.into(),
            status: SessionStatus::Finalizing,
            text: None,
            output_file: None,
            error: None,
        }
    }

    fn completed(id: &str, text: Option<&str>) -> RecorderSession {
        RecorderSession {
            id: id.into(),
            status: SessionStatus::Completed,
            text: text.map(str::to_owned),
            output_file: Some("/tmp/r.wav".into()),
            error: None,
        }
    }

    fn failed(id: &str, error: Option<&str>) -> RecorderSession {
        RecorderSession {
            id: id.into(),
            status: SessionStatus::Failed,
            text: None,
            output_file: Some("/tmp/r.wav".into()),
            error: error.map(str::to_owned),
        }
    }

    fn http_409() -> ApiError {
        ApiError::Http {
            status: 409,
            message: "Already recording".into(),
        }
    }

    #[test]
    fn happy_path_toggle_stop_poll_complete() {
        let t0 = Instant::now();
        let api = MockApi::new()
            .start(Ok(recording("a")))
            .stop(Ok(finalizing("a")))
            .session(Ok(finalizing("a")))
            .session(Ok(completed("a", Some("hi"))));
        let mut engine = Engine::new(api);

        assert!(matches!(engine.toggle(t0), Update::None));
        assert!(
            matches!(engine.state(), State::Recording { id, started } if id == "a" && *started == t0)
        );
        assert_eq!(engine.started_at(), Some(t0));

        let stopped_at = t0 + Duration::from_secs(5);
        assert!(matches!(engine.toggle(stopped_at), Update::None));
        assert!(matches!(engine.state(), State::Finalizing { id, deadline }
                if id == "a" && *deadline == stopped_at + Duration::from_secs(120)));

        assert!(matches!(
            engine.tick(stopped_at + Duration::from_secs(1)),
            Update::None
        ));
        assert!(matches!(engine.state(), State::Finalizing { .. }));

        match engine.tick(stopped_at + Duration::from_secs(2)) {
            Update::Transcribed { text, output_file } => {
                assert_eq!(text.as_deref(), Some("hi"));
                assert_eq!(output_file.as_deref(), Some("/tmp/r.wav"));
            }
            other => panic!("expected Transcribed, got {other:?}"),
        }
        assert!(matches!(engine.state(), State::Idle));
        assert_eq!(engine.api.start_calls, 1);
        assert_eq!(engine.api.stop_calls, 1);
        assert_eq!(engine.api.session_calls, 2);
    }

    #[test]
    fn start_error_enters_error_and_next_toggle_retries() {
        let t0 = Instant::now();
        let api = MockApi::new()
            .start(Err(http_409()))
            .start(Ok(recording("b")));
        let mut engine = Engine::new(api);

        match engine.toggle(t0) {
            Update::Error(message) => assert!(message.contains("Already recording")),
            other => panic!("expected Error, got {other:?}"),
        }
        assert!(
            matches!(engine.state(), State::Error { message } if message.contains("Already recording"))
        );
        assert!(engine.last_error().unwrap().contains("Already recording"));

        assert!(matches!(
            engine.toggle(t0 + Duration::from_secs(1)),
            Update::None
        ));
        assert!(matches!(engine.state(), State::Recording { id, .. } if id == "b"));
        assert_eq!(engine.last_error(), None);
        assert_eq!(engine.api.start_calls, 2);
    }

    #[test]
    fn failed_session_surfaces_provider_error() {
        let t0 = Instant::now();
        let api = MockApi::new()
            .start(Ok(recording("a")))
            .stop(Ok(finalizing("a")))
            .session(Ok(failed("a", Some("finalTranscription: boom"))));
        let mut engine = Engine::new(api);

        let stopped_at = t0 + Duration::from_secs(1);
        engine.toggle(t0);
        engine.toggle(stopped_at);

        match engine.tick(stopped_at + Duration::from_secs(1)) {
            Update::Error(message) => assert_eq!(message, "finalTranscription: boom"),
            other => panic!("expected Error, got {other:?}"),
        }
        assert!(
            matches!(engine.state(), State::Error { message } if message == "finalTranscription: boom")
        );
    }

    #[test]
    fn timeout_produces_error() {
        let t0 = Instant::now();
        let api = MockApi::new()
            .start(Ok(recording("a")))
            .stop(Ok(finalizing("a")));
        let mut engine = Engine::new(api);
        engine.toggle(t0);
        engine.toggle(t0);

        let deadline = t0 + Duration::from_secs(120);
        match engine.tick(deadline + Duration::from_secs(1)) {
            Update::Error(message) => {
                assert!(message.contains("timed out"), "message was {message:?}")
            }
            other => panic!("expected Error, got {other:?}"),
        }
        assert!(matches!(engine.state(), State::Error { .. }));
        assert_eq!(engine.api.session_calls, 0);
    }

    #[test]
    fn toggle_during_finalizing_is_ignored() {
        let t0 = Instant::now();
        let api = MockApi::new()
            .start(Ok(recording("a")))
            .stop(Ok(finalizing("a")));
        let mut engine = Engine::new(api);
        engine.toggle(t0);
        engine.toggle(t0);

        let calls = (
            engine.api.start_calls,
            engine.api.stop_calls,
            engine.api.session_calls,
        );
        assert!(matches!(
            engine.toggle(t0 + Duration::from_secs(5)),
            Update::None
        ));
        assert_eq!(
            (
                engine.api.start_calls,
                engine.api.stop_calls,
                engine.api.session_calls
            ),
            calls
        );
        assert!(matches!(engine.state(), State::Finalizing { .. }));
    }

    #[test]
    fn empty_text_is_transcribed_with_none_text() {
        let t0 = Instant::now();
        let api = MockApi::new()
            .start(Ok(recording("a")))
            .stop(Ok(finalizing("a")))
            .session(Ok(completed("a", None)));
        let mut engine = Engine::new(api);
        engine.toggle(t0);
        engine.toggle(t0);

        match engine.tick(t0 + Duration::from_secs(1)) {
            Update::Transcribed { text, output_file } => {
                assert_eq!(text, None);
                assert_eq!(output_file.as_deref(), Some("/tmp/r.wav"));
            }
            other => panic!("expected Transcribed, got {other:?}"),
        }
    }

    #[test]
    fn deadline_is_at_least_120s() {
        let t0 = Instant::now();
        let api = MockApi::new()
            .start(Ok(recording("a")))
            .stop(Ok(finalizing("a")))
            .session(Ok(finalizing("a")));
        let mut engine = Engine::new(api);
        engine.toggle(t0);
        engine.toggle(t0);

        assert!(matches!(
            engine.tick(t0 + Duration::from_secs(119)),
            Update::None
        ));
        assert!(matches!(engine.state(), State::Finalizing { .. }));
        assert_eq!(engine.last_error(), None);
    }
}
