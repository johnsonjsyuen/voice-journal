//! Voice Journal core library.
//!
//! The platform-neutral modules in this crate compile and test on every
//! platform. The macOS GUI layer lives in `platform/macos` behind
//! `cfg(target_os = "macos")`.

pub mod api;
pub mod check;
pub mod config;
pub mod discovery;
pub mod engine;
pub mod journal;
#[cfg(target_os = "macos")]
pub mod platform;
