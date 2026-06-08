use std::sync::{Mutex, OnceLock};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimingEntry {
    pub label: String,
    pub millis: u128,
}

#[derive(Debug)]
struct TimingState {
    enabled: bool,
    entries: Vec<TimingEntry>,
    last_time: Instant,
}

static TIMINGS: OnceLock<Mutex<TimingState>> = OnceLock::new();

fn state() -> &'static Mutex<TimingState> {
    TIMINGS.get_or_init(|| {
        Mutex::new(TimingState {
            enabled: std::env::var("PI_TIMING").is_ok_and(|value| value == "1"),
            entries: Vec::new(),
            last_time: Instant::now(),
        })
    })
}

pub fn reset_timings() {
    let mut state = state().lock().expect("timing lock should not be poisoned");
    if !state.enabled {
        return;
    }
    state.entries.clear();
    state.last_time = Instant::now();
}

pub fn time(label: impl Into<String>) {
    let mut state = state().lock().expect("timing lock should not be poisoned");
    if !state.enabled {
        return;
    }
    let now = Instant::now();
    let millis = now.duration_since(state.last_time).as_millis();
    state.entries.push(TimingEntry {
        label: label.into(),
        millis,
    });
    state.last_time = now;
}

pub fn timing_entries() -> Vec<TimingEntry> {
    state()
        .lock()
        .expect("timing lock should not be poisoned")
        .entries
        .clone()
}

pub fn format_timings() -> Option<String> {
    let entries = timing_entries();
    if entries.is_empty() {
        return None;
    }
    let total = entries.iter().map(|entry| entry.millis).sum::<u128>();
    let mut lines = vec!["--- Startup Timings ---".to_string()];
    for entry in entries {
        lines.push(format!("  {}: {}ms", entry.label, entry.millis));
    }
    lines.push(format!("  TOTAL: {total}ms"));
    lines.push("------------------------".to_string());
    Some(lines.join("\n"))
}
