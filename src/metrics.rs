//! In-memory journaling metrics rendered in the tray menu.

use chrono::{DateTime, Local, NaiveDate};
use std::time::Duration;

pub const MAX_LAST_CHARS: usize = 48;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Metrics {
    day: Option<NaiveDate>,
    recordings: u32,
    total: Duration,
    last_text: Option<String>,
    last_duration: Option<Duration>,
}

impl Metrics {
    pub fn record(&mut self, text: Option<&str>, duration: Duration, now: DateTime<Local>) {
        let day = now.date_naive();
        if self.day != Some(day) {
            self.day = Some(day);
            self.recordings = 0;
            self.total = Duration::ZERO;
        }
        self.recordings += 1;
        self.total += duration;
        self.last_duration = Some(duration);
        if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
            self.last_text = Some(text.trim().to_owned());
        }
    }

    pub fn recordings(&self) -> u32 {
        self.recordings
    }

    pub fn total_duration(&self) -> Duration {
        self.total
    }

    pub fn last_text(&self) -> Option<&str> {
        self.last_text.as_deref()
    }

    pub fn last_duration(&self) -> Option<Duration> {
        self.last_duration
    }
}

pub fn last_line(metrics: &Metrics) -> String {
    let text = match metrics.last_text() {
        Some(text) => format!("\"{}\"", truncate(text, MAX_LAST_CHARS)),
        None => "—".into(),
    };
    match metrics.last_duration() {
        Some(duration) => format!("Last: {text} ({})", format_duration(duration)),
        None => format!("Last: {text}"),
    }
}

pub fn today_line(metrics: &Metrics) -> String {
    match metrics.recordings() {
        0 => "Today: no recordings".into(),
        count => format!(
            "Today: {count} {} · {}",
            if count == 1 {
                "recording"
            } else {
                "recordings"
            },
            format_duration(metrics.total_duration())
        ),
    }
}

pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        if minutes == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {minutes}m")
        }
    } else if minutes > 0 {
        if seconds == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m {seconds}s")
        }
    } else {
        format!("{seconds}s")
    }
}

pub fn truncate(text: &str, max_chars: usize) -> String {
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
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn record_tracks_last_text_and_duration() {
        let mut metrics = Metrics::default();
        metrics.record(
            Some("hello"),
            Duration::from_secs(30),
            at(2026, 9, 12, 9, 0),
        );

        assert_eq!(metrics.last_text(), Some("hello"));
        assert_eq!(metrics.last_duration(), Some(Duration::from_secs(30)));
        assert_eq!(metrics.recordings(), 1);
        assert_eq!(metrics.total_duration(), Duration::from_secs(30));
    }

    #[test]
    fn silent_recording_counts_but_keeps_previous_last_text() {
        let mut metrics = Metrics::default();
        metrics.record(
            Some("hello"),
            Duration::from_secs(10),
            at(2026, 9, 12, 9, 0),
        );
        metrics.record(None, Duration::from_secs(5), at(2026, 9, 12, 9, 1));

        assert_eq!(metrics.last_text(), Some("hello"));
        assert_eq!(metrics.last_duration(), Some(Duration::from_secs(5)));
        assert_eq!(metrics.recordings(), 2);
        assert_eq!(metrics.total_duration(), Duration::from_secs(15));
    }

    #[test]
    fn counters_reset_on_a_new_day() {
        let mut metrics = Metrics::default();
        metrics.record(
            Some("late"),
            Duration::from_secs(60),
            at(2026, 9, 11, 23, 59),
        );
        metrics.record(
            Some("early"),
            Duration::from_secs(30),
            at(2026, 9, 12, 0, 1),
        );

        assert_eq!(metrics.recordings(), 1);
        assert_eq!(metrics.total_duration(), Duration::from_secs(30));
        assert_eq!(metrics.last_text(), Some("early"));
    }

    #[test]
    fn lines_render_last_and_today() {
        let mut metrics = Metrics::default();
        assert_eq!(last_line(&metrics), "Last: —");
        assert_eq!(today_line(&metrics), "Today: no recordings");

        metrics.record(
            Some("refactored the prompt handling"),
            Duration::from_secs(23 * 60 + 41),
            at(2026, 9, 12, 14, 0),
        );
        assert_eq!(
            last_line(&metrics),
            "Last: \"refactored the prompt handling\" (23m 41s)"
        );
        assert_eq!(today_line(&metrics), "Today: 1 recording · 23m 41s");
    }

    #[test]
    fn last_line_shows_duration_without_text() {
        let mut metrics = Metrics::default();
        metrics.record(None, Duration::from_secs(5), at(2026, 9, 12, 14, 0));

        assert_eq!(last_line(&metrics), "Last: — (5s)");
    }

    #[test]
    fn today_line_pluralizes_recordings() {
        let mut metrics = Metrics::default();
        for _ in 0..2 {
            metrics.record(Some("x"), Duration::from_secs(1), at(2026, 9, 12, 14, 0));
        }
        assert!(today_line(&metrics).starts_with("Today: 2 recordings · "));
    }

    #[test]
    fn long_last_text_is_truncated() {
        let mut metrics = Metrics::default();
        let long = "x".repeat(100);
        metrics.record(Some(&long), Duration::from_secs(1), at(2026, 9, 12, 14, 0));

        let line = last_line(&metrics);
        assert!(line.ends_with("…\" (1s)"));
        assert_eq!(
            line.chars().count(),
            "Last: \"".chars().count() + MAX_LAST_CHARS + "…\" (1s)".chars().count()
        );
    }

    #[test]
    fn format_duration_ranges() {
        assert_eq!(format_duration(Duration::from_secs(0)), "0s");
        assert_eq!(format_duration(Duration::from_secs(41)), "41s");
        assert_eq!(format_duration(Duration::from_secs(120)), "2m");
        assert_eq!(
            format_duration(Duration::from_secs(23 * 60 + 41)),
            "23m 41s"
        );
        assert_eq!(format_duration(Duration::from_secs(3600)), "1h");
        assert_eq!(format_duration(Duration::from_secs(3660)), "1h 1m");
        assert_eq!(
            format_duration(Duration::from_secs(3600 + 23 * 60)),
            "1h 23m"
        );
    }
}
