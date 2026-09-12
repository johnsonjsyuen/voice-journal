//! Global hotkey registration.

use global_hotkey::GlobalHotKeyManager;
use global_hotkey::hotkey::HotKey;
use std::str::FromStr;

pub const EXAMPLE: &str = "Ctrl+Alt+KeyJ";

pub struct HotkeyHandle {
    _manager: GlobalHotKeyManager,
    pub hotkey: HotKey,
}

pub fn register(spec: &str) -> Result<HotkeyHandle, String> {
    let hotkey = parse(spec)?;
    let manager = GlobalHotKeyManager::new()
        .map_err(|e| format!("failed to create global hotkey manager: {e}"))?;
    manager
        .register(hotkey)
        .map_err(|e| format!("failed to register hotkey {spec:?}: {e}"))?;
    Ok(HotkeyHandle {
        _manager: manager,
        hotkey,
    })
}

pub fn parse(spec: &str) -> Result<HotKey, String> {
    HotKey::from_str(spec)
        .map_err(|e| format!("invalid hotkey {spec:?}: {e} (accepted example: {EXAMPLE})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_default_example() {
        assert!(parse(EXAMPLE).is_ok());
    }

    #[test]
    fn parse_error_mentions_accepted_example() {
        let error = parse("Shift+Ctrl").unwrap_err();
        assert!(error.contains(EXAMPLE), "error was {error:?}");
    }
}
