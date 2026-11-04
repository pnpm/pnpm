//! The warnings a declaration earns from its manifest alone, so a package
//! warns, or does not, on every machine and in every CI job.

use super::VariantDeclaration;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use serde_json::Value;
use std::path::Path;

/// Warn about `declaration` of the package at `dir`, whose base manifest is
/// `base`: a declaration without an entry that targets `any` publishes a
/// version that has no archive for a host no variant matches, and platform
/// variants without a top-level `files` ship every unclaimed file in every
/// archive.
pub(crate) fn warn_about_declaration<Reporter: self::Reporter>(
    dir: &Path,
    base: &Value,
    declaration: &VariantDeclaration,
) {
    if declaration.primary().is_none() {
        warn::<Reporter>(
            dir,
            "publishConfig.variants has no entry that targets \"any\": the version has no \
             archive for a host that no variant matches, so npm and other clients that do not \
             read variants install an empty package that reports an unsupported platform. Add \
             an entry with \"targets\": [\"any\"] to keep the package installable everywhere.",
        );
    }
    let has_platform_variants = declaration.entries
        .iter()
        .any(|entry| !entry.is_any());
    // The packlist reads a `files` that is not an array as none.
    if has_platform_variants
        && base
            .get("files")
            .and_then(Value::as_array)
            .is_none()
    {
        warn::<Reporter>(
            dir,
            "publishConfig.variants declares platform variants, but the package has no \
             \"files\" field, so every file that no entry claims ships in every archive. List \
             what the archives share in \"files\"; \"files\": [] shares nothing.",
        );
    }
}

fn warn<Reporter: self::Reporter>(dir: &Path, message: &str) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: message.to_string(),
        prefix: dir.to_string_lossy().into_owned(),
    }));
}
