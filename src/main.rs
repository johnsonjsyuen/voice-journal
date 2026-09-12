const USAGE: &str = "Usage: voice-journal [--check | --version | --help]\n\n  (no args)  run the macOS menu-bar daemon\n  --check    validate config, journal writability, and TypeWhisper discovery/API\n  --version  print version\n  --help     print this help\n";

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,voice_journal=info"),
    )
    .init();
    match std::env::args().nth(1).as_deref() {
        Some("--check") => std::process::exit(voice_journal::check::run_cli()),
        Some("--version") | Some("-V") => {
            println!("voice-journal {}", env!("CARGO_PKG_VERSION"));
        }
        Some("--help") | Some("-h") => print!("{USAGE}"),
        Some(other) => {
            eprintln!("unknown argument: {other}\n{USAGE}");
            std::process::exit(2);
        }
        None => run_daemon(),
    }
}

#[cfg(target_os = "macos")]
fn run_daemon() {
    if let Err(e) = voice_journal::platform::macos::run() {
        eprintln!("fatal: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn run_daemon() {
    eprintln!("voice-journal daemon runs on macOS only (use --check here)");
    std::process::exit(1);
}
