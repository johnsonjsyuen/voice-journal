use std::time::{Duration, Instant};

use chrono::TimeZone;
use httpmock::prelude::*;
use voice_journal::api::HttpApi;
use voice_journal::engine::{self, Engine, State, Update};
use voice_journal::journal;

#[test]
fn recorder_flow_appends_transcript_to_journal() {
    let server = MockServer::start();

    let start_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/recorder/start")
            .query_param("mic", "true")
            .query_param("system_audio", "false");
        then.status(200)
            .body(r#"{"id":"sess-1","status":"recording"}"#);
    });
    let stop_mock = server.mock(|when, then| {
        when.method(POST).path("/v1/recorder/stop");
        then.status(200)
            .body(r#"{"id":"sess-1","status":"finalizing"}"#);
    });
    let session_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/recorder/session")
            .query_param("id", "sess-1");
        then.status(200)
            .body(r#"{"id":"sess-1","status":"completed","text":"hello journal"}"#);
    });

    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("VoiceJournal.md");

    let api = HttpApi::new(server.port(), None);
    let mut engine = Engine::new(api);

    let t0 = Instant::now();
    assert!(matches!(engine.toggle(t0), Update::None));
    assert!(matches!(engine.state(), State::Recording { id, .. } if id == "sess-1"));

    assert!(matches!(
        engine.toggle(t0 + Duration::from_secs(3)),
        Update::None
    ));
    assert!(matches!(engine.state(), State::Finalizing { id, .. } if id == "sess-1"));

    let text = match engine.tick(t0 + Duration::from_secs(4)) {
        Update::Transcribed {
            text: Some(text), ..
        } => text,
        other => panic!("expected Transcribed with text, got {other:?}"),
    };
    assert_eq!(text, "hello journal");
    assert!(matches!(engine.state(), State::Idle));

    let now = chrono::Local
        .with_ymd_and_hms(2026, 9, 11, 14, 32, 0)
        .unwrap();
    journal::append_entry(&journal_path, &text, now).unwrap();

    assert_eq!(
        std::fs::read_to_string(&journal_path).unwrap(),
        "# Voice Journal\n\n- 2026-09-11 14:32 — hello journal\n"
    );

    start_mock.assert_hits(1);
    stop_mock.assert_hits(1);
    session_mock.assert_hits(1);
}

#[test]
fn quit_during_recording_appends_final_transcript() {
    let server = MockServer::start();

    let start_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/recorder/start")
            .query_param("mic", "true")
            .query_param("system_audio", "false");
        then.status(200)
            .body(r#"{"id":"sess-2","status":"recording"}"#);
    });
    let stop_mock = server.mock(|when, then| {
        when.method(POST).path("/v1/recorder/stop");
        then.status(200)
            .body(r#"{"id":"sess-2","status":"finalizing"}"#);
    });
    let session_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/recorder/session")
            .query_param("id", "sess-2");
        then.status(200)
            .body(r#"{"id":"sess-2","status":"completed","text":"last words"}"#);
    });

    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("VoiceJournal.md");

    let mut engine = Engine::new(HttpApi::new(server.port(), None));
    let t0 = Instant::now();
    assert!(matches!(engine.toggle(t0), Update::None));
    assert!(matches!(engine.state(), State::Recording { id, .. } if id == "sess-2"));

    let stopped_at = t0 + Duration::from_secs(1);
    let mut final_update = None;
    engine::shutdown(
        &mut engine,
        t0 + Duration::from_secs(30),
        || stopped_at,
        |_| {},
        |update, _| {
            if matches!(update, Update::Transcribed { .. }) {
                final_update = Some(update);
            }
        },
    );

    let (text, duration) = match final_update.expect("shutdown must emit the final transcript") {
        Update::Transcribed {
            text: Some(text),
            duration,
            ..
        } => (text, duration),
        other => panic!("expected Transcribed with text, got {other:?}"),
    };
    assert_eq!(text, "last words");
    assert_eq!(duration, Duration::from_secs(1));

    let now = chrono::Local
        .with_ymd_and_hms(2026, 9, 11, 14, 32, 0)
        .unwrap();
    journal::append_entry(&journal_path, &text, now).unwrap();

    assert_eq!(
        std::fs::read_to_string(&journal_path).unwrap(),
        "# Voice Journal\n\n- 2026-09-11 14:32 — last words\n"
    );

    start_mock.assert_hits(1);
    stop_mock.assert_hits(1);
    session_mock.assert_hits(1);
}
