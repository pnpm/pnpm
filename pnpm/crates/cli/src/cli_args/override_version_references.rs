//! The `$dep-name` self-reference syntax in `overrides` — a value that
//! copies the specifier the root manifest declares for that dependency —
//! is deprecated in favor of catalogs. pnpm still honors it, and warns
//! once per command about every selector still using it.

use pacquet_config::Config;
use pacquet_reporter::{GlobalLog, LogEvent, LogLevel};
use serde_json::Value;

/// Warn about the `overrides` selectors whose configured value is a
/// `$dep-name` reference.
///
/// The values [`Config::overrides`] carries are already resolved, so
/// the raw ones are read back from [`Config::explicit_settings`], the
/// record of what each config source set.
pub(crate) fn warn_deprecated_override_version_references(config: &Config, emit: fn(&LogEvent)) {
    let Some(overrides) = config.explicit_settings.get("overrides").and_then(Value::as_object)
    else {
        return;
    };
    let selectors = overrides
        .iter()
        .filter(|(_, spec)| spec.as_str().is_some_and(|spec| spec.starts_with('$')))
        .map(|(selector, _)| selector.as_str())
        .collect::<Vec<_>>();
    if selectors.is_empty() {
        return;
    }
    emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Warn,
        message: format!(
            "The \"$\" version reference syntax in overrides is deprecated (used by: {}). \
             Define the version in a catalog and reference it with the \"catalog:\" protocol \
             instead. See https://pnpm.io/catalogs",
            selectors.join(", "),
        ),
    }));
}
