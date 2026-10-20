//! The [`SOURCE_DATE_EPOCH`](https://reproducible-builds.org/specs/source-date-epoch/)
//! convention for reproducible output.

use crate::api::{EnvVar, Host};
use std::{
    sync::OnceLock,
    time::{Duration, SystemTime},
};

/// The time `SOURCE_DATE_EPOCH` sets, read once per process. `None` when the
/// variable is unset or is not a whole number of seconds since the epoch.
pub fn source_date_epoch() -> Option<SystemTime> {
    static SOURCE_DATE_EPOCH: OnceLock<Option<SystemTime>> = OnceLock::new();
    *SOURCE_DATE_EPOCH.get_or_init(read_source_date_epoch::<Host>)
}

pub(crate) fn read_source_date_epoch<Sys: EnvVar>() -> Option<SystemTime> {
    let seconds = Sys::var("SOURCE_DATE_EPOCH")?
        .trim()
        .parse::<u64>()
        .ok()?;
    if seconds > i64::MAX as u64 / 1_000 {
        return None;
    }
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests;
