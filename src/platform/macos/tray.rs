//! Menu-bar tray icon, menu, and programmatically rendered icon states.

use crate::engine::State;
use std::path::Path;
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{BadIcon, Icon, TrayIcon, TrayIconBuilder};

pub const MENU_ID_STATUS: &str = "status";
pub const MENU_ID_OPEN_JOURNAL: &str = "open-journal";
pub const MENU_ID_OPEN_CONFIG: &str = "open-config";
pub const MENU_ID_QUIT: &str = "quit";

pub const ICON_SIZE_16: u32 = 16;
pub const ICON_SIZE_32: u32 = 32;

const ICON_SIZE: u32 = ICON_SIZE_32;
const SAMPLES_PER_AXIS: u32 = 4;
const MAX_STATUS_CHARS: usize = 80;
const MAX_TOOLTIP_CHARS: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayState {
    Idle,
    NoSpeech,
    Recording,
    Finalizing,
    Error(String),
}

impl TrayState {
    pub fn from_engine(state: &State) -> Self {
        match state {
            State::Idle => Self::Idle,
            State::Recording { .. } => Self::Recording,
            State::Finalizing { .. } => Self::Finalizing,
            State::Error { message } => Self::Error(message.clone()),
        }
    }

    pub fn is_template(&self) -> bool {
        matches!(self, Self::Idle | Self::NoSpeech)
    }
}

pub fn status_text(state: &TrayState) -> String {
    match state {
        TrayState::Idle | TrayState::NoSpeech => "Status: Idle".into(),
        TrayState::Recording => "Status: Recording".into(),
        TrayState::Finalizing => "Status: Finalizing".into(),
        TrayState::Error(message) => {
            format!("Status: Error: {}", truncate(message, MAX_STATUS_CHARS))
        }
    }
}

pub fn tooltip_text(state: &TrayState) -> String {
    match state {
        TrayState::Idle => "Voice Journal — Idle".into(),
        TrayState::NoSpeech => "No speech detected".into(),
        TrayState::Recording => "Voice Journal — Recording".into(),
        TrayState::Finalizing => "Voice Journal — Finalizing".into(),
        TrayState::Error(message) => format!(
            "Voice Journal — Error: {}",
            truncate(message, MAX_TOOLTIP_CHARS)
        ),
    }
}

pub fn rgba_icon(state: &TrayState, size: u32) -> Vec<u8> {
    let color = match state {
        TrayState::Idle | TrayState::NoSpeech => [0, 0, 0],
        TrayState::Recording | TrayState::Finalizing => [255, 59, 48],
        TrayState::Error(_) => [255, 149, 0],
    };
    let square = matches!(state, TrayState::Error(_));
    rasterize(size, color, move |x, y| {
        if square {
            return true;
        }
        let center = size as f64 / 2.0;
        let radius = center - 1.0;
        let dx = x - center;
        let dy = y - center;
        dx * dx + dy * dy <= radius * radius
    })
}

pub fn icon(state: &TrayState, size: u32) -> Result<Icon, BadIcon> {
    Icon::from_rgba(rgba_icon(state, size), size, size)
}

pub fn open_path(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("/usr/bin/open")
        .arg(path)
        .spawn()
        .map(|_| ())
}

pub struct Tray {
    icon: TrayIcon,
    status_item: MenuItem,
}

impl Tray {
    pub fn new() -> Result<Self, String> {
        let idle = TrayState::Idle;
        let status_item = MenuItem::with_id(MENU_ID_STATUS, status_text(&idle), false, None);
        let menu = Menu::new();
        menu.append_items(&[
            &status_item,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(MENU_ID_OPEN_JOURNAL, "Open Journal…", true, None),
            &MenuItem::with_id(MENU_ID_OPEN_CONFIG, "Open Config…", true, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(MENU_ID_QUIT, "Quit", true, None),
        ])
        .map_err(|e| format!("failed to build tray menu: {e}"))?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon(&idle, ICON_SIZE).map_err(|e| format!("bad tray icon: {e}"))?)
            .with_icon_as_template(idle.is_template())
            .with_tooltip(tooltip_text(&idle))
            .build()
            .map_err(|e| format!("failed to create tray icon: {e}"))?;

        Ok(Self {
            icon: tray_icon,
            status_item,
        })
    }

    pub fn set_state(&self, state: &TrayState) {
        match icon(state, ICON_SIZE) {
            Ok(icon) => {
                if let Err(e) = self
                    .icon
                    .set_icon_with_as_template(Some(icon), state.is_template())
                {
                    log::error!("failed to update tray icon: {e}");
                }
            }
            Err(e) => log::error!("failed to render tray icon: {e}"),
        }
        if let Err(e) = self.icon.set_tooltip(Some(tooltip_text(state))) {
            log::error!("failed to update tray tooltip: {e}");
        }
        self.status_item.set_text(status_text(state));
    }
}

fn rasterize(size: u32, color: [u8; 3], inside: impl Fn(f64, f64) -> bool) -> Vec<u8> {
    let mut rgba = vec![0u8; (size as usize) * (size as usize) * 4];
    let step = 1.0 / SAMPLES_PER_AXIS as f64;
    let sample_count = SAMPLES_PER_AXIS * SAMPLES_PER_AXIS;
    for y in 0..size {
        for x in 0..size {
            let mut hits = 0u32;
            for sy in 0..SAMPLES_PER_AXIS {
                for sx in 0..SAMPLES_PER_AXIS {
                    let px = x as f64 + (sx as f64 + 0.5) * step;
                    let py = y as f64 + (sy as f64 + 0.5) * step;
                    if inside(px, py) {
                        hits += 1;
                    }
                }
            }
            let offset = ((y * size + x) * 4) as usize;
            rgba[offset] = color[0];
            rgba[offset + 1] = color[1];
            rgba[offset + 2] = color[2];
            rgba[offset + 3] = (hits * 255 / sample_count) as u8;
        }
    }
    rgba
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let mut shortened: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        shortened.push('…');
    }
    shortened
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states() -> [TrayState; 5] {
        [
            TrayState::Idle,
            TrayState::NoSpeech,
            TrayState::Recording,
            TrayState::Finalizing,
            TrayState::Error("boom".into()),
        ]
    }

    #[test]
    fn icon_rgba_has_four_bytes_per_pixel() {
        for state in states() {
            for size in [ICON_SIZE_16, ICON_SIZE_32] {
                assert_eq!(rgba_icon(&state, size).len(), (size * size * 4) as usize);
            }
        }
    }

    #[test]
    fn circle_corners_are_transparent_and_square_corners_are_opaque() {
        let circle = rgba_icon(&TrayState::Recording, ICON_SIZE_16);
        assert_eq!(circle[3], 0, "circle corner alpha");
        let square = rgba_icon(&TrayState::Error("boom".into()), ICON_SIZE_16);
        assert_eq!(square[3], 255, "square corner alpha");
    }

    #[test]
    fn only_idle_icons_are_templates() {
        assert!(TrayState::Idle.is_template());
        assert!(TrayState::NoSpeech.is_template());
        assert!(!TrayState::Recording.is_template());
        assert!(!TrayState::Finalizing.is_template());
        assert!(!TrayState::Error("boom".into()).is_template());
    }

    #[test]
    fn status_text_reports_state_and_error() {
        assert_eq!(status_text(&TrayState::Idle), "Status: Idle");
        assert_eq!(status_text(&TrayState::Recording), "Status: Recording");
        assert_eq!(status_text(&TrayState::Finalizing), "Status: Finalizing");
        assert!(status_text(&TrayState::Error("boom".into())).contains("boom"));
    }

    #[test]
    fn no_speech_tooltip_is_exact() {
        assert_eq!(tooltip_text(&TrayState::NoSpeech), "No speech detected");
    }

    #[test]
    fn truncate_keeps_short_text_and_marks_long_text() {
        assert_eq!(truncate("hello", 80), "hello");
        let shortened = truncate(&"x".repeat(100), 80);
        assert_eq!(shortened.chars().count(), 81);
        assert!(shortened.ends_with('…'));
    }
}
