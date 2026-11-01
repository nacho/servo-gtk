/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Lightweight, opt-in metrics for benchmarking.
//!
//! Metrics are disabled by default and cost one relaxed atomic load per call
//! site when off. They are enabled by setting the `SERVO_GTK_METRICS`
//! environment variable (see [`output_target`]) or by calling
//! [`set_enabled`] before the first `WebView` is created. The UI process
//! forwards the enabled state to the runner subprocess it spawns.
//!
//! Each process keeps its own registry of named histograms and counters.
//! Histograms record raw `u64` samples (durations in microseconds by
//! convention, with names ending in `_us`) and summarise them into
//! count/sum/mean/min/max/percentiles on [`snapshot_json`].
//!
//! Naming convention: `<process>.<area>.<what>[_us]`, e.g.
//! `runner.frame.readback_us`, `ui.input.to_frame_us`, `ui.ipc.frame_bytes`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Environment variable that enables metrics and selects the output target.
pub const ENV_VAR: &str = "SERVO_GTK_METRICS";
/// Environment variable overriding the periodic flush interval, in seconds.
/// `0` disables periodic flushing.
pub const INTERVAL_ENV_VAR: &str = "SERVO_GTK_METRICS_INTERVAL";
/// Version of the JSON report layout.
pub const SCHEMA_VERSION: u64 = 1;

/// Hard cap on the samples kept per histogram, to bound memory on long runs.
/// Once reached, further samples still update count/sum/min/max but are not
/// kept for percentile computation.
const MAX_SAMPLES: usize = 1 << 20;

static ENABLED: OnceLock<AtomicBool> = OnceLock::new();
static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

fn enabled_flag() -> &'static AtomicBool {
    ENABLED.get_or_init(|| {
        let on = std::env::var_os(ENV_VAR).is_some_and(|v| !v.is_empty() && v != "0");
        AtomicBool::new(on)
    })
}

/// Whether metrics collection is enabled in this process.
#[inline]
pub fn enabled() -> bool {
    enabled_flag().load(Ordering::Relaxed)
}

/// Force metrics on or off in this process. Call before creating a `WebView`
/// so the runner subprocess inherits the setting.
pub fn set_enabled(on: bool) {
    enabled_flag().store(on, Ordering::Relaxed);
}

/// Where the UI process writes the merged report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputTarget {
    /// Do not write a report (metrics may still be read via the API).
    None,
    /// Pretty JSON to stderr.
    Stderr,
    /// Pretty JSON to this file, replaced atomically on each flush.
    File(std::path::PathBuf),
}

/// Parse `SERVO_GTK_METRICS`: unset/empty/`0` → `None`; `stderr` → `Stderr`;
/// anything else is a file path. `1` is accepted as an alias for `stderr` so
/// the common `VAR=1` way of enabling a feature doesn't write a file named `1`.
pub fn output_target() -> OutputTarget {
    match std::env::var_os(ENV_VAR) {
        None => OutputTarget::None,
        Some(v) if v.is_empty() || v == "0" => OutputTarget::None,
        Some(v) if v == "stderr" || v == "1" => OutputTarget::Stderr,
        Some(v) => OutputTarget::File(v.into()),
    }
}

/// Periodic flush interval from `SERVO_GTK_METRICS_INTERVAL` (default 5 s).
/// `None` means periodic flushing is disabled.
pub fn flush_interval() -> Option<Duration> {
    let secs = std::env::var(INTERVAL_ENV_VAR)
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(5.0);
    (secs.is_finite() && secs > 0.0).then(|| Duration::from_secs_f64(secs))
}

/// Microseconds on the shared CLOCK_MONOTONIC clock, comparable between the UI
/// and runner processes on the same host.
#[inline]
pub fn monotonic_us() -> i64 {
    glib::monotonic_time()
}

#[derive(Default)]
struct Histogram {
    count: u64,
    sum: u128,
    min: u64,
    max: u64,
    samples: Vec<u64>,
}

impl Histogram {
    fn record(&mut self, value: u64) {
        if self.count == 0 || value < self.min {
            self.min = value;
        }
        if value > self.max {
            self.max = value;
        }
        self.count += 1;
        self.sum += value as u128;
        if self.samples.len() < MAX_SAMPLES {
            self.samples.push(value);
        }
    }

    fn summary(&self) -> serde_json::Value {
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let pct = |p: f64| -> u64 {
            if sorted.is_empty() {
                return 0;
            }
            // Nearest-rank percentile.
            let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
            sorted[rank.clamp(1, sorted.len()) - 1]
        };
        let mean = if self.count == 0 {
            0.0
        } else {
            self.sum as f64 / self.count as f64
        };
        serde_json::json!({
            "count": self.count,
            "sum": self.sum as u64,
            "mean": (mean * 10.0).round() / 10.0,
            "min": self.min,
            "p50": pct(50.0),
            "p90": pct(90.0),
            "p95": pct(95.0),
            "p99": pct(99.0),
            "max": self.max,
        })
    }
}

struct Registry {
    started: Instant,
    histograms: BTreeMap<&'static str, Histogram>,
    counters: BTreeMap<&'static str, u64>,
}

impl Registry {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            histograms: BTreeMap::new(),
            counters: BTreeMap::new(),
        }
    }
}

fn with_registry<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
    let mut guard = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Registry::new))
}

/// Record a raw sample into histogram `name`. No-op when disabled.
#[inline]
pub fn record(name: &'static str, value: u64) {
    if enabled() {
        with_registry(|r| r.histograms.entry(name).or_default().record(value));
    }
}

/// Record a duration (in microseconds) into histogram `name`.
#[inline]
pub fn record_duration(name: &'static str, duration: Duration) {
    if enabled() {
        record(name, duration.as_micros().min(u64::MAX as u128) as u64);
    }
}

/// Add `n` to counter `name`. No-op when disabled.
#[inline]
pub fn count(name: &'static str, n: u64) {
    if enabled() {
        with_registry(|r| *r.counters.entry(name).or_default() += n);
    }
}

/// Start a timer: `Some(Instant::now())` when enabled, `None` otherwise, so
/// the clock is not read at all when metrics are off. Pair with [`stop`].
#[inline]
pub fn start() -> Option<Instant> {
    enabled().then(Instant::now)
}

/// Record the time since `start` (from [`start`]) into histogram `name`.
#[inline]
pub fn stop(name: &'static str, start: Option<Instant>) {
    if let Some(start) = start {
        record_duration(name, start.elapsed());
    }
}

/// RAII timer recording its lifetime into histogram `name` on drop.
pub struct ScopeTimer {
    name: &'static str,
    start: Option<Instant>,
}

impl Drop for ScopeTimer {
    fn drop(&mut self) {
        stop(self.name, self.start);
    }
}

/// Time the enclosing scope into histogram `name`.
#[inline]
pub fn scope(name: &'static str) -> ScopeTimer {
    ScopeTimer {
        name,
        start: start(),
    }
}

/// Clear all histograms and counters and restart the uptime clock.
pub fn reset() {
    let mut guard = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(Registry::new());
}

/// Snapshot this process's metrics as a JSON object:
/// `{ "uptime_s": f64, "counters": {name: u64}, "histograms": {name: summary} }`.
pub fn snapshot_json() -> serde_json::Value {
    with_registry(|r| {
        let histograms: serde_json::Map<String, serde_json::Value> = r
            .histograms
            .iter()
            .map(|(k, h)| (k.to_string(), h.summary()))
            .collect();
        let counters: serde_json::Map<String, serde_json::Value> = r
            .counters
            .iter()
            .map(|(k, v)| (k.to_string(), (*v).into()))
            .collect();
        serde_json::json!({
            "uptime_s": r.started.elapsed().as_secs_f64(),
            "counters": counters,
            "histograms": histograms,
        })
    })
}

/// Build the merged report written by the UI process.
pub fn build_report(ui: serde_json::Value, runner: Option<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "schema": SCHEMA_VERSION,
        "pid": std::process::id(),
        "ui": ui,
        "runner": runner.unwrap_or(serde_json::Value::Null),
    })
}

/// Write `report` to the configured [`output_target`]. File output goes to a
/// temporary sibling first and is renamed into place, so readers never see a
/// partial file.
pub fn write_report(report: &serde_json::Value) {
    let text = match serde_json::to_string_pretty(report) {
        Ok(t) => t,
        Err(_) => return,
    };
    match output_target() {
        OutputTarget::None => {}
        OutputTarget::Stderr => eprintln!("{text}"),
        OutputTarget::File(path) => {
            let mut tmp = path.clone().into_os_string();
            tmp.push(".tmp");
            let result =
                std::fs::write(&tmp, text.as_bytes()).and_then(|()| std::fs::rename(&tmp, &path));
            if let Err(e) = result {
                eprintln!("servo-gtk: failed to write metrics to {path:?}: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_summary_percentiles() {
        let mut h = Histogram::default();
        for v in 1..=100 {
            h.record(v);
        }
        let s = h.summary();
        assert_eq!(s["count"], 100);
        assert_eq!(s["min"], 1);
        assert_eq!(s["max"], 100);
        assert_eq!(s["p50"], 50);
        assert_eq!(s["p95"], 95);
        assert_eq!(s["p99"], 99);
        assert_eq!(s["mean"], 50.5);
    }

    #[test]
    fn empty_histogram_summary_is_zero() {
        let s = Histogram::default().summary();
        assert_eq!(s["count"], 0);
        assert_eq!(s["p50"], 0);
    }

    #[test]
    fn single_sample_percentiles() {
        let mut h = Histogram::default();
        h.record(7);
        let s = h.summary();
        assert_eq!(s["p50"], 7);
        assert_eq!(s["p99"], 7);
    }
}
