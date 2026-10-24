//! The `tracing` subscriber behind the `log` config.

use pnpr_config::{LogConfig, LogFormat};
use tracing::Subscriber;
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

/// Builds the subscriber for `logs.format` writing to `writer`.
///
/// Every event emitted inside a request carries the request span's fields.
/// The JSON format nests them under `span` and drops the entered-span list,
/// so each line is one flat record.
pub fn log_subscriber<Writer>(
    logs: &LogConfig,
    filter: EnvFilter,
    writer: Writer,
    ansi: bool,
) -> Box<dyn Subscriber + Send + Sync>
where
    Writer: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(ansi);
    match logs.format {
        LogFormat::Json => Box::new(
            builder
                .json()
                .with_current_span(true)
                .with_span_list(false)
                .finish(),
        ),
        LogFormat::Pretty => Box::new(builder.compact().finish()),
    }
}
