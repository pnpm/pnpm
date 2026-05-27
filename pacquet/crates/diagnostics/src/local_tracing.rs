use std::str::FromStr;

use tracing::Level;
use tracing_subscriber::{EnvFilter, Layer, fmt::format::FmtSpan};

pub fn enable_tracing_by_env() {
    let Ok(trace_var) = std::env::var("TRACE") else { return };

    use tracing_subscriber::{fmt, prelude::*};
    let layer = common_layer(&trace_var);

    tracing_subscriber::registry()
        .with(layer)
        .with(fmt::layer().pretty().with_file(true).with_span_events(FmtSpan::CLOSE))
        .init();

    tracing::trace!("enable_tracing_by_env");
}

fn common_layer(trace_var: &str) -> Box<dyn Layer<tracing_subscriber::Registry> + Send + Sync> {
    if let Ok(default_level) = Level::from_str(trace_var) {
        tracing_subscriber::filter::Targets::new()
            .with_target("pacquet_tarball", default_level)
            .boxed()
    } else {
        // SAFETY: for the `expect`, if we can't parse the directive, then the tracing result would be
        // unexpected, then panic is reasonable
        EnvFilter::builder()
            .with_regex(true)
            .parse(trace_var)
            .expect("Parse tracing directive syntax failed,for details about the directive syntax you could refer https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#directives")
            .boxed()
    }
}
