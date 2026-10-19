use std::{
    fmt::Write,
    sync::atomic::{AtomicI64, AtomicU64, Ordering},
    time::Duration,
};

use axum::{
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use dashmap::DashMap;

use super::AppState;

pub(crate) const DURATION_BUCKETS: [f64; 11] =
    [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

pub(crate) struct InFlightGuard<'a>(&'a AtomicI64);

impl<'a> InFlightGuard<'a> {
    pub(crate) fn new(counter: &'a AtomicI64) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

struct DurationHistogram {
    buckets: [AtomicU64; 11],
    sum_micros: AtomicU64,
    count: AtomicU64,
}

impl DurationHistogram {
    fn new() -> Self {
        Self {
            buckets: Default::default(),
            sum_micros: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    fn record(&self, duration: Duration) {
        let secs = duration.as_secs_f64();
        for (index, &bound) in DURATION_BUCKETS.iter().enumerate() {
            if secs <= bound {
                self.buckets[index].fetch_add(1, Ordering::Relaxed);
            }
        }
        self.sum_micros.fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }
}

fn normalize_method(method: &str) -> &'static str {
    match method {
        "GET" => "GET",
        "HEAD" => "HEAD",
        "POST" => "POST",
        "PUT" => "PUT",
        "DELETE" => "DELETE",
        "PATCH" => "PATCH",
        "OPTIONS" => "OPTIONS",
        _ => "OTHER",
    }
}

pub(crate) struct ServerMetrics {
    pub(crate) in_flight: AtomicI64,
    requests_total: DashMap<(&'static str, u16), AtomicU64>,
    duration: DurationHistogram,
    response_bytes_total: AtomicU64,
    cache_hits_total: AtomicU64,
    cache_misses_total: AtomicU64,
    login_rate_limits_total: AtomicU64,
}

impl ServerMetrics {
    pub(crate) fn new() -> Self {
        Self {
            in_flight: AtomicI64::new(0),
            requests_total: DashMap::new(),
            duration: DurationHistogram::new(),
            response_bytes_total: AtomicU64::new(0),
            cache_hits_total: AtomicU64::new(0),
            cache_misses_total: AtomicU64::new(0),
            login_rate_limits_total: AtomicU64::new(0),
        }
    }

    pub(crate) fn start_in_flight(&self) -> InFlightGuard<'_> {
        InFlightGuard::new(&self.in_flight)
    }

    pub(crate) fn record_request(&self, method: &str, status: u16, duration: Duration, bytes: u64) {
        let key = (normalize_method(method), status);
        if let Some(counter) = self.requests_total.get(&key) {
            counter.fetch_add(1, Ordering::Relaxed);
        } else {
            self.requests_total
                .entry(key)
                .or_insert_with(|| AtomicU64::new(0))
                .fetch_add(1, Ordering::Relaxed);
        }

        self.duration.record(duration);
        self.response_bytes_total.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(crate) fn record_cache_hit(&self) {
        self.cache_hits_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cache_miss(&self) {
        self.cache_misses_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_login_throttled(&self) {
        self.login_rate_limits_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn render_prometheus(&self) -> String {
        let mut buffer = String::with_capacity(2048);

        writeln!(buffer, "# HELP pnpr_build_info Information about the pnpr build.").unwrap();
        writeln!(buffer, "# TYPE pnpr_build_info gauge").unwrap();
        writeln!(buffer, "pnpr_build_info{{version=\"{}\"}} 1\n", env!("CARGO_PKG_VERSION"))
            .unwrap();

        writeln!(
            buffer,
            "# HELP pnpr_http_requests_in_flight Current number of HTTP requests being processed.",
        )
        .unwrap();
        writeln!(buffer, "# TYPE pnpr_http_requests_in_flight gauge").unwrap();
        let in_flight = self.in_flight.load(Ordering::Relaxed).max(0);
        writeln!(buffer, "pnpr_http_requests_in_flight {in_flight}\n").unwrap();

        self.render_requests_total(&mut buffer);
        self.render_duration_histogram(&mut buffer);

        writeln!(buffer, "# HELP pnpr_http_response_bytes_total Total response bytes served.")
            .unwrap();
        writeln!(buffer, "# TYPE pnpr_http_response_bytes_total counter").unwrap();
        let bytes = self.response_bytes_total.load(Ordering::Relaxed);
        writeln!(buffer, "pnpr_http_response_bytes_total {bytes}\n").unwrap();

        self.render_counters(&mut buffer);
        buffer
    }

    fn render_requests_total(&self, buffer: &mut String) {
        writeln!(
            buffer,
            "# HELP pnpr_http_requests_total Total number of HTTP requests processed.",
        )
        .unwrap();
        writeln!(buffer, "# TYPE pnpr_http_requests_total counter").unwrap();
        let mut entries: Vec<_> = self.requests_total
            .iter()
            .map(|entry| (*entry.key(), entry.value().load(Ordering::Relaxed)))
            .collect();
        entries.sort_by(|entry_a, entry_b| entry_a.0.cmp(&entry_b.0));
        for ((method, status), count) in entries {
            writeln!(
                buffer,
                r#"pnpr_http_requests_total{{method="{method}",status="{status}"}} {count}"#,
            )
            .unwrap();
        }
        buffer.push('\n');
    }

    fn render_duration_histogram(&self, buffer: &mut String) {
        writeln!(
            buffer,
            "# HELP pnpr_http_request_duration_seconds HTTP request latencies in seconds.",
        )
        .unwrap();
        writeln!(buffer, "# TYPE pnpr_http_request_duration_seconds histogram").unwrap();
        for (index, &bound) in DURATION_BUCKETS.iter().enumerate() {
            let count = self.duration.buckets[index].load(Ordering::Relaxed);
            writeln!(
                buffer,
                r#"pnpr_http_request_duration_seconds_bucket{{le="{bound}"}} {count}"#,
            )
            .unwrap();
        }
        let total = self.duration.count.load(Ordering::Relaxed);
        writeln!(buffer, r#"pnpr_http_request_duration_seconds_bucket{{le="+Inf"}} {total}"#)
            .unwrap();
        let sum = (self.duration.sum_micros.load(Ordering::Relaxed) as f64) / 1_000_000.0;
        writeln!(buffer, "pnpr_http_request_duration_seconds_sum {sum:.6}").unwrap();
        writeln!(buffer, "pnpr_http_request_duration_seconds_count {total}\n").unwrap();
    }

    fn render_counters(&self, buffer: &mut String) {
        writeln!(buffer, "# HELP pnpr_cache_hits_total Total proxy cache hits.").unwrap();
        writeln!(buffer, "# TYPE pnpr_cache_hits_total counter").unwrap();
        let hits = self.cache_hits_total.load(Ordering::Relaxed);
        writeln!(buffer, "pnpr_cache_hits_total {hits}\n").unwrap();

        writeln!(buffer, "# HELP pnpr_cache_misses_total Total proxy cache misses.").unwrap();
        writeln!(buffer, "# TYPE pnpr_cache_misses_total counter").unwrap();
        let misses = self.cache_misses_total.load(Ordering::Relaxed);
        writeln!(buffer, "pnpr_cache_misses_total {misses}\n").unwrap();

        writeln!(
            buffer,
            "# HELP pnpr_login_rate_limits_total Total login requests throttled by rate limit.",
        )
        .unwrap();
        writeln!(buffer, "# TYPE pnpr_login_rate_limits_total counter").unwrap();
        let rate_limits = self.login_rate_limits_total.load(Ordering::Relaxed);
        writeln!(buffer, "pnpr_login_rate_limits_total {rate_limits}").unwrap();
    }
}

/// Serve Prometheus metrics in standard text format.
///
/// Unauthenticated access is intentional to allow telemetry scrapers (such as
/// Prometheus) to collect server metrics without credentials.
pub(crate) async fn serve_metrics(State(state): State<AppState>) -> Response {
    let rendered = state.inner.metrics.render_prometheus();
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")], rendered)
        .into_response()
}
