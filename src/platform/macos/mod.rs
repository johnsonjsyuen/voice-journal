//! macOS daemon wiring: winit event loop, global hotkey, tray, and engine worker.

pub mod hotkey;
pub mod tray;

use crate::api::HttpApi;
use crate::engine::{Engine, State, Update};
use crate::{config, discovery, journal};
use chrono::Local;
use fs4::TryLockError;
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant};
use tray_icon::TrayIconEvent;
use tray_icon::menu::MenuEvent;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
use winit::window::WindowId;

const UI_TICK: Duration = Duration::from_millis(100);
const WORKER_TICK: Duration = Duration::from_millis(250);

enum WorkerCommand {
    Toggle,
    Quit,
}

struct WorkerEvent {
    update: Update,
    state: Option<State>,
}

enum Notice {
    NoSpeech,
    Error(String),
}

pub fn run() -> Result<(), String> {
    let config_path = config::config_path().map_err(|e| e.to_string())?;
    let lock = acquire_single_instance_lock(&config_path)?;
    let cfg = config::load_or_create(&config_path).map_err(|e| e.to_string())?;
    hotkey::parse(&cfg.hotkey)?;

    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    spawn_worker(command_rx, event_tx);

    let mut builder = EventLoop::builder();
    builder.with_activation_policy(ActivationPolicy::Accessory);
    let event_loop = builder
        .build()
        .map_err(|e| format!("failed to create event loop: {e}"))?;

    let mut app = App {
        tray: None,
        hotkey: None,
        hotkey_spec: cfg.hotkey,
        commands: command_tx,
        events: event_rx,
        journal_path: cfg.journal_path,
        config_path,
        notice: None,
        error: None,
        _lock: lock,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("event loop error: {e}"))?;
    match app.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn acquire_single_instance_lock(config_path: &Path) -> Result<File, String> {
    let dir = config_path
        .parent()
        .ok_or("config path has no parent directory")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let path = dir.join("adapter.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    fs4::FileExt::try_lock(&file).map_err(|e| match e {
        TryLockError::WouldBlock => "voice-journal is already running".to_string(),
        TryLockError::Error(e) => format!("cannot lock {}: {e}", path.display()),
    })?;
    Ok(file)
}

fn load_discovery_api() -> Result<HttpApi, String> {
    let path = discovery::discovery_path().map_err(|e| e.to_string())?;
    let discovery = discovery::load(&path).map_err(|e| e.to_string())?;
    Ok(HttpApi::from_discovery(&discovery, path))
}

fn spawn_worker(commands: Receiver<WorkerCommand>, events: Sender<WorkerEvent>) {
    std::thread::spawn(move || {
        let mut engine: Option<Engine<HttpApi>> = None;
        loop {
            match commands.recv_timeout(WORKER_TICK) {
                Ok(WorkerCommand::Toggle) => {
                    let api = match load_discovery_api() {
                        Ok(api) => api,
                        Err(message) => {
                            let event = WorkerEvent {
                                update: Update::Error(message),
                                state: None,
                            };
                            if events.send(event).is_err() {
                                break;
                            }
                            continue;
                        }
                    };
                    let rebuild = engine.as_ref().is_none_or(|engine| {
                        matches!(engine.state(), State::Idle | State::Error { .. })
                    });
                    if rebuild {
                        engine = Some(Engine::new(api));
                    }
                    let Some(engine) = engine.as_mut() else {
                        continue;
                    };
                    let update = engine.toggle(Instant::now());
                    let state = Some(engine.state().clone());
                    if events.send(WorkerEvent { update, state }).is_err() {
                        break;
                    }
                }
                Ok(WorkerCommand::Quit) => std::process::exit(0),
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(engine) = engine.as_mut() {
                        let update = engine.tick(Instant::now());
                        let state = Some(engine.state().clone());
                        if events.send(WorkerEvent { update, state }).is_err() {
                            break;
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

fn copy_to_clipboard(text: &str) {
    let mut child = match Command::new("/usr/bin/pbcopy")
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            log::error!("failed to launch pbcopy: {e}");
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(text.as_bytes())
    {
        log::error!("failed to write transcript to pbcopy: {e}");
    }
    if let Err(e) = child.wait() {
        log::error!("pbcopy failed: {e}");
    }
}

struct App {
    tray: Option<tray::Tray>,
    hotkey: Option<hotkey::HotkeyHandle>,
    hotkey_spec: String,
    commands: Sender<WorkerCommand>,
    events: Receiver<WorkerEvent>,
    journal_path: PathBuf,
    config_path: PathBuf,
    notice: Option<Notice>,
    error: Option<String>,
    _lock: File,
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: String) {
        log::error!("{error}");
        self.error = Some(error);
        event_loop.exit();
    }

    fn drain_hotkeys(&mut self) {
        for event in GlobalHotKeyEvent::receiver().try_iter() {
            let matches_hotkey = self
                .hotkey
                .as_ref()
                .is_some_and(|handle| event.id == handle.hotkey.id());
            if matches_hotkey && event.state == HotKeyState::Pressed {
                let _ = self.commands.send(WorkerCommand::Toggle);
            }
        }
    }

    fn drain_menu(&mut self) {
        for event in MenuEvent::receiver().try_iter() {
            if event.id() == tray::MENU_ID_OPEN_JOURNAL {
                if let Err(e) = tray::open_path(&self.journal_path) {
                    log::error!("failed to open journal: {e}");
                }
            } else if event.id() == tray::MENU_ID_OPEN_CONFIG {
                if let Err(e) = tray::open_path(&self.config_path) {
                    log::error!("failed to open config: {e}");
                }
            } else if event.id() == tray::MENU_ID_QUIT {
                let _ = self.commands.send(WorkerCommand::Quit);
            }
        }
    }

    fn drain_worker(&mut self) {
        loop {
            match self.events.try_recv() {
                Ok(event) => self.handle_worker_event(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    log::error!("engine worker stopped");
                    break;
                }
            }
        }
    }

    fn handle_worker_event(&mut self, event: WorkerEvent) {
        match event.update {
            Update::Transcribed { text, .. } => match text {
                Some(text) if !text.trim().is_empty() => self.append_transcript(&text),
                _ => self.notice = Some(Notice::NoSpeech),
            },
            Update::Error(message) => self.notice = Some(Notice::Error(message)),
            Update::None => {}
        }

        if let Some(state) = event.state {
            if matches!(state, State::Recording { .. } | State::Finalizing { .. }) {
                self.notice = None;
            }
            if let Some(tray) = &self.tray {
                tray.set_state(&self.tray_state(&state));
            }
        }
    }

    fn append_transcript(&mut self, text: &str) {
        match journal::append_entry(&self.journal_path, text, Local::now()) {
            Ok(()) => self.notice = None,
            Err(e) => {
                copy_to_clipboard(text);
                self.notice = Some(Notice::Error(format!(
                    "journal write failed; transcript copied to clipboard: {e}"
                )));
            }
        }
    }

    fn tray_state(&self, state: &State) -> tray::TrayState {
        match &self.notice {
            Some(Notice::Error(message)) => tray::TrayState::Error(message.clone()),
            Some(Notice::NoSpeech) if matches!(state, State::Idle) => tray::TrayState::NoSpeech,
            _ => tray::TrayState::from_engine(state),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.tray.is_none() {
            match tray::Tray::new() {
                Ok(tray) => self.tray = Some(tray),
                Err(e) => {
                    self.fail(event_loop, e);
                    return;
                }
            }
        }
        if self.hotkey.is_none() {
            match hotkey::register(&self.hotkey_spec) {
                Ok(handle) => self.hotkey = Some(handle),
                Err(e) => {
                    self.fail(event_loop, e);
                    return;
                }
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + UI_TICK));
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.drain_hotkeys();
        self.drain_menu();
        for _ in TrayIconEvent::receiver().try_iter() {}
        self.drain_worker();
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + UI_TICK));
    }
}
