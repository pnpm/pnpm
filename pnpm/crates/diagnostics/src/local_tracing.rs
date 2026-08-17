use std::str::FromStr;

use tracing::Level;
use tracing_subscriber::{EnvFilter, Layer, fmt::format::FmtSpan};

pub fn enable_tracing_by_env() {
    let Ok(trace_var) = std::env::var("TRACE") else { return };

    use tracing_subscriber::{fmt, prelude::*};
    // Never panic here: this runs at `@pnpm/napi` module load (and CLI
    // startup), where a malformed `TRACE` value or an already-installed
    // subscriber must degrade to "tracing stays off", not abort the host
    // process.
    let Some(layer) = common_layer(&trace_var) else {
        eprintln!(
            "ignoring invalid TRACE filter {trace_var:?}; see \
             https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#directives",
        );
        return;
    };
    let init_result = if std::env::var("TRACE_FORMAT").is_ok_and(|format| format == "json") {
        tracing_subscriber::registry()
            .with(layer)
            .with(fmt::layer().json().flatten_event(true))
            .try_init()
    } else {
        tracing_subscriber::registry()
            .with(layer)
            .with(fmt::layer().pretty().with_file(true).with_span_events(FmtSpan::CLOSE))
            .try_init()
    };
    // A subscriber installed earlier in the process keeps precedence.
    if init_result.is_err() {
        return;
    }

    tracing::trace!("enable_tracing_by_env");
}

fn common_layer(
    trace_var: &str,
) -> Option<Box<dyn Layer<tracing_subscriber::Registry> + Send + Sync>> {
    if let Ok(default_level) = Level::from_str(trace_var) {
        Some(
            tracing_subscriber::filter::Targets::new()
                .with_target("pnpm_tarball", default_level)
                .boxed(),
        )
    } else {
        EnvFilter::builder().with_regex(true).parse(trace_var).ok().map(Layer::boxed)
    }
}
