use super::{ByteSize, Deserialize, Interval, RegistryError, parse_byte_size, parse_interval};
use std::time::Duration;

/// How pnpr serves its own package document,
/// `application/vnd.pnpr.install-v2+json`, to clients that ask for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageDocumentsConfig {
    /// The size of a package's abbreviated document, in bytes of serialized
    /// JSON, above which only the versions a resolution likely picks keep
    /// their manifests inline. 64 KiB by default.
    pub split_threshold: u64,
    /// How far back from a package's last publish its versions keep their
    /// publish times. Never shorter than [`Self::MIN_PUBLISH_TIME_WINDOW`].
    pub publish_time_window: Duration,
}

impl PackageDocumentsConfig {
    /// The longest minimum release age a client may apply to the document:
    /// every version older than the window is old enough for it.
    pub const MIN_PUBLISH_TIME_WINDOW: Duration = Duration::from_hours(30 * 24);
    pub const DEFAULT_SPLIT_THRESHOLD: u64 = 64 * 1024;
}

impl Default for PackageDocumentsConfig {
    fn default() -> Self {
        Self {
            split_threshold: Self::DEFAULT_SPLIT_THRESHOLD,
            publish_time_window: Self::MIN_PUBLISH_TIME_WINDOW,
        }
    }
}

/// Disk shape of the `packageDocuments:` block.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PackageDocumentsFile {
    #[serde(default)]
    split_threshold: Option<ByteSize>,
    #[serde(default)]
    publish_time_window: Option<Interval>,
}

pub(super) fn build_package_documents_config(
    file: Option<PackageDocumentsFile>,
) -> Result<PackageDocumentsConfig, RegistryError> {
    let file = file.unwrap_or_default();
    let defaults = PackageDocumentsConfig::default();
    let split_threshold = match file.split_threshold {
        Some(ByteSize(raw)) => parse_byte_size(&raw)
            .ok_or_else(|| {
                invalid(format!("packageDocuments.splitThreshold {raw:?} is not a valid byte size"))
            })?,
        None => defaults.split_threshold,
    };
    let publish_time_window = match file.publish_time_window {
        Some(Interval(raw)) => parse_interval(&raw)
            .ok_or_else(|| {
                invalid(format!(
                    "packageDocuments.publishTimeWindow {raw:?} is not a valid interval"
                ))
            })?,
        None => defaults.publish_time_window,
    };
    if publish_time_window < PackageDocumentsConfig::MIN_PUBLISH_TIME_WINDOW {
        return Err(invalid(
            "packageDocuments.publishTimeWindow must be at least 30d: clients accept a minimum release age of up to 30 days".to_string(),
        ));
    }
    Ok(PackageDocumentsConfig { split_threshold, publish_time_window })
}

fn invalid(reason: String) -> RegistryError {
    RegistryError::InvalidConfig { reason }
}
