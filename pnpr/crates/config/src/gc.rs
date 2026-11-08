use super::{Deserialize, Interval, RegistryError, parse_interval};
use std::time::Duration;

/// How `pnpr gc` reclaims unreferenced archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcConfig {
    /// How long a reclaim that `pnpr gc` (or another replica) started and did
    /// not finish is left alone before another run takes it over. It affects
    /// only how soon interrupted reclaims finish, never safety.
    pub lease: Duration,
}

impl GcConfig {
    pub const DEFAULT_LEASE: Duration = Duration::from_hours(1);
}

impl Default for GcConfig {
    fn default() -> Self {
        Self { lease: Self::DEFAULT_LEASE }
    }
}

/// Disk shape of the `gc:` block.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GcFile {
    #[serde(default)]
    lease: Option<Interval>,
}

pub(super) fn build_gc_config(file: Option<GcFile>) -> Result<GcConfig, RegistryError> {
    let Some(Interval(raw)) = file.unwrap_or_default().lease else {
        return Ok(GcConfig::default());
    };
    let lease = parse_interval(&raw)
        .ok_or_else(|| invalid(format!("gc.lease {raw:?} is not a valid interval")))?;
    if lease.is_zero() {
        return Err(invalid(format!("gc.lease {raw:?} must be greater than zero")));
    }
    Ok(GcConfig { lease })
}

fn invalid(reason: String) -> RegistryError {
    RegistryError::InvalidConfig { reason }
}
