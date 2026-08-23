//! The `updateNotifier` setting: tell the user when a newer pnpm exists.
//!
//! `pnpm install` and `pnpm add` ask the registry once a day for the
//! version behind pnpm's `latest` tag and emit it on `pnpm:update-check`;
//! the default reporter prints a notice when that version is newer than the
//! running one. The daily cadence is kept in
//! `<stateDir>/pnpm-state.json` under `lastUpdateCheck`, the same file and
//! key pnpm writes.
//!
//! The whole check is best-effort: an unreadable state file, an
//! unreachable registry, or an unwritable state directory leaves the
//! command's own outcome untouched.
//!
//! An `install` that finishes through the repeat-install fast path never
//! reaches this module — that path deliberately returns before the async
//! runtime and the HTTP client exist. The check runs on the next install
//! that has work to do.

use crate::cli_args::registry_client::build_registry_client;
use chrono::{DateTime, Utc};
use pnpm_config::{Config, PNPM_VERSION};
use pnpm_fs::write_atomic;
use pnpm_registry::Package;
use pnpm_reporter::{LogEvent, LogLevel, UpdateCheckLog};
use pnpm_resolving_npm_resolver::pick_registry_for_package;
use serde_json::{Map, Value};
use std::{collections::HashMap, path::Path};
use tokio::task::JoinHandle;

/// How long a recorded check suppresses the next one.
const UPDATE_CHECK_FREQUENCY_MS: i64 = 24 * 60 * 60 * 1000;

const STATE_FILE_NAME: &str = "pnpm-state.json";
const LAST_UPDATE_CHECK_KEY: &str = "lastUpdateCheck";

/// The running command's update check, if one is due.
///
/// [`settle`] disposes of it; dropping it without settling detaches the
/// task, which would leave the notice racing process exit.
pub(crate) type PendingUpdateCheck = Option<JoinHandle<()>>;

/// Start the daily update check in the background, or return [`None`] when
/// the settings turn it off, the run is offline, or today's check already
/// happened.
///
/// `config` must already carry the command's CLI overrides, since
/// `--offline` and `--prefer-offline` are among the things that call the
/// check off.
pub(crate) fn spawn(config: &Config, emit: fn(&LogEvent)) -> PendingUpdateCheck {
    if !config.update_notifier || config.ci || config.offline || config.prefer_offline {
        return None;
    }
    let state_file = config.state_dir.join(STATE_FILE_NAME);
    let state = read_state(&state_file);
    if checked_recently(&state, Utc::now()) {
        return None;
    }
    let config = config.clone();
    Some(tokio::spawn(async move {
        check(&config, &state_file, state, emit).await;
    }))
}

/// Dispose of the check [`spawn`] started, now that the command it ran
/// alongside has an outcome.
///
/// A command that succeeded waits, so the notice reaches the terminal
/// before the process exits. A command that failed drops the check
/// instead: the error is what the user needs to read, and nothing was
/// recorded, so the next command checks again.
pub(crate) async fn settle(pending: PendingUpdateCheck, outcome: &miette::Result<()>) {
    let Some(handle) = pending else { return };
    if outcome.is_ok() {
        let _ = handle.await;
    } else {
        handle.abort();
    }
}

async fn check(config: &Config, state_file: &Path, state: Map<String, Value>, emit: fn(&LogEvent)) {
    // A registry that cannot be reached is not worth a word to the user:
    // they asked for an install, not for a version check. The cadence is
    // left untouched too, so the next command retries.
    let Ok(latest_version) = latest_pnpm_version(config).await else {
        return;
    };
    if let Some(latest_version) = latest_version {
        emit(&LogEvent::UpdateCheck(UpdateCheckLog {
            level: LogLevel::Debug,
            current_version: PNPM_VERSION.to_string(),
            latest_version,
        }));
    }
    write_state(state_file, state, Utc::now());
}

/// The version behind pnpm's `latest` tag on the registry the project
/// installs from — the same registry `pnpm add pnpm` would reach, not the
/// trusted bootstrap registry `self-update` provisions the engine from. A
/// registry that serves pnpm without a `latest` tag answers `None`.
async fn latest_pnpm_version(config: &Config) -> miette::Result<Option<String>> {
    let client = build_registry_client(config)?;
    let registries: HashMap<String, String> = config.resolved_registries().into_iter().collect();
    let registry = pick_registry_for_package(&registries, "pnpm", None);
    let package = Package::fetch_from_registry("pnpm", &client, &registry, &config.auth_headers)
        .await
        .map_err(|error| miette::miette!("{error}"))?;
    Ok(package.dist_tags.get("latest").cloned())
}

fn read_state(state_file: &Path) -> Map<String, Value> {
    match std::fs::read_to_string(state_file).ok().map(|text| serde_json::from_str(&text)) {
        Some(Ok(Value::Object(fields))) => fields,
        _ => Map::new(),
    }
}

/// Whether the last recorded check is younger than
/// [`UPDATE_CHECK_FREQUENCY_MS`].
///
/// A missing, unparsable, or future timestamp reads as "never checked", so
/// a corrupted state file — or a clock that moved backwards — makes pnpm
/// check again rather than go quiet until the recorded time comes around.
fn checked_recently(state: &Map<String, Value>, now: DateTime<Utc>) -> bool {
    state
        .get(LAST_UPDATE_CHECK_KEY)
        .and_then(Value::as_str)
        .and_then(|last| DateTime::parse_from_rfc2822(last).ok())
        .map(|last| (now - last.with_timezone(&Utc)).num_milliseconds())
        .is_some_and(|age| (0..UPDATE_CHECK_FREQUENCY_MS).contains(&age))
}

/// Record the check, keeping every other key the file carries.
///
/// Written through [`write_atomic`], as pnpm writes it through
/// `write-file-atomic`: a crash mid-write cannot leave the file truncated
/// for the next run to read, and the rename replaces a symlinked state file
/// rather than following it somewhere the user never pointed pnpm.
fn write_state(state_file: &Path, mut state: Map<String, Value>, now: DateTime<Utc>) {
    state.insert(LAST_UPDATE_CHECK_KEY.to_string(), Value::String(to_utc_string(now)));
    let Ok(contents) = serde_json::to_string(&Value::Object(state)) else {
        return;
    };
    let _ = write_atomic(state_file, contents.as_bytes());
}

/// JavaScript's `Date#toUTCString`, the format pnpm writes the timestamp in.
fn to_utc_string(time: DateTime<Utc>) -> String {
    time.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

#[cfg(test)]
mod tests;
