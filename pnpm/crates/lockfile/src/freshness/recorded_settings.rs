//! The lockfile's `settings:` block and `pnpmfileChecksum` against the
//! current configuration.

use super::{PnpmfileChecksumCheck, ResolutionSettingsCheck, StalenessReason};
use crate::{Lockfile, LockfileSettings, ResolutionSettings};

impl StalenessReason {
    /// [`StalenessReason::setting_name`] for the reasons that compare a key of
    /// the lockfile's `settings:` block.
    pub(super) fn settings_block_key(&self) -> Option<&'static str> {
        match self {
            StalenessReason::AutoInstallPeersChanged { .. } => Some("settings.autoInstallPeers"),
            StalenessReason::DedupePeersChanged { .. } => Some("settings.dedupePeers"),
            StalenessReason::ImplicitTypesPeersChanged { .. } => {
                Some("settings.implicitTypesPeers")
            }
            StalenessReason::ExcludeLinksFromLockfileChanged { .. } => {
                Some("settings.excludeLinksFromLockfile")
            }
            StalenessReason::PeersSuffixMaxLengthChanged { .. } => {
                Some("settings.peersSuffixMaxLength")
            }
            StalenessReason::InjectWorkspacePackagesChanged { .. } => {
                Some("settings.injectWorkspacePackages")
            }
            _ => None,
        }
    }
}

/// The `settings:` block plus the pnpmfile checksum. A lockfile with no
/// `settings` block records nothing about the settings it was written under,
/// so there is nothing to compare — pnpm's
/// `lockfile.settings?.autoInstallPeers != null` guard.
pub(super) fn check_recorded_settings(
    lockfile: &Lockfile,
    check: &ResolutionSettingsCheck<'_>,
) -> Result<(), StalenessReason> {
    let settings = lockfile.settings.as_ref();
    check_peer_settings(settings, check)?;

    if let Some(settings) = settings
        && exclude_links_from_lockfile_changed(Some(settings), check.exclude_links_from_lockfile)
    {
        return Err(StalenessReason::ExcludeLinksFromLockfileChanged {
            lockfile: settings.exclude_links_from_lockfile,
            config: check.exclude_links_from_lockfile,
        });
    }

    let lockfile_peers_suffix_max_length = recorded_peers_suffix_max_length(settings);
    if lockfile_peers_suffix_max_length != check.peers_suffix_max_length {
        return Err(StalenessReason::PeersSuffixMaxLengthChanged {
            lockfile: lockfile_peers_suffix_max_length,
            config: check.peers_suffix_max_length,
        });
    }

    check_pnpmfile_checksum(lockfile, &check.pnpmfile_checksum)?;

    let lockfile_inject = recorded_inject_workspace_packages(settings);
    if lockfile_inject != check.inject_workspace_packages {
        return Err(StalenessReason::InjectWorkspacePackagesChanged {
            lockfile: lockfile_inject,
            config: check.inject_workspace_packages,
        });
    }

    check_resolution_settings(settings, check.resolution_settings)
}

fn check_peer_settings(
    settings: Option<&LockfileSettings>,
    check: &ResolutionSettingsCheck<'_>,
) -> Result<(), StalenessReason> {
    if let Some(settings) = settings
        && auto_install_peers_changed(Some(settings), check.auto_install_peers)
    {
        return Err(StalenessReason::AutoInstallPeersChanged {
            lockfile: settings.auto_install_peers,
            config: check.auto_install_peers,
        });
    }

    let lockfile_dedupe_peers = recorded_dedupe_peers(settings);
    if lockfile_dedupe_peers != check.dedupe_peers {
        return Err(StalenessReason::DedupePeersChanged {
            lockfile: lockfile_dedupe_peers,
            config: check.dedupe_peers,
        });
    }

    let lockfile_implicit_types_peers = recorded_implicit_types_peers(settings);
    if lockfile_implicit_types_peers != check.implicit_types_peers {
        return Err(StalenessReason::ImplicitTypesPeersChanged {
            lockfile: lockfile_implicit_types_peers,
            config: check.implicit_types_peers,
        });
    }
    Ok(())
}

fn check_resolution_settings(
    settings: Option<&LockfileSettings>,
    expected: Option<&ResolutionSettings>,
) -> Result<(), StalenessReason> {
    let Some(expected) = expected else { return Ok(()) };
    let unrecorded = ResolutionSettings::default();
    let recorded = settings.map_or(&unrecorded, |settings| &settings.resolution);
    match recorded.first_difference(expected) {
        None => Ok(()),
        Some(difference) => Err(StalenessReason::ResolutionSettingChanged {
            setting: difference.setting,
            lockfile: describe_setting_value(difference.recorded.as_ref()),
            config: describe_setting_value(difference.expected.as_ref()),
        }),
    }
}

fn describe_setting_value(value: Option<&serde_json::Value>) -> String {
    value.map_or_else(|| "unset".to_string(), ToString::to_string)
}

fn check_pnpmfile_checksum(
    lockfile: &Lockfile,
    checksum: &PnpmfileChecksumCheck<'_>,
) -> Result<(), StalenessReason> {
    if let PnpmfileChecksumCheck::Current(pnpmfile_checksum) = checksum
        && lockfile.pnpmfile_checksum.as_deref() != *pnpmfile_checksum
    {
        return Err(StalenessReason::PnpmfileChecksumChanged {
            lockfile: lockfile.pnpmfile_checksum.clone(),
            config: pnpmfile_checksum.map(str::to_string),
        });
    }

    Ok(())
}

/// Whether `settings.autoInstallPeers` drifted from what the lockfile
/// records. A lockfile with no `settings` block records nothing about
/// the setting it was written under, so there is nothing to compare.
///
/// This and its peers below are the single definition of "this
/// lockfile setting changed", shared with the fast path that records a
/// provably inert setting change without re-resolving.
#[must_use]
pub fn auto_install_peers_changed(
    recorded: Option<&LockfileSettings>,
    auto_install_peers: bool,
) -> bool {
    recorded.is_some_and(|settings| settings.auto_install_peers != auto_install_peers)
}

/// See [`auto_install_peers_changed`].
#[must_use]
pub fn exclude_links_from_lockfile_changed(
    recorded: Option<&LockfileSettings>,
    exclude_links_from_lockfile: bool,
) -> bool {
    recorded.is_some_and(|settings| {
        settings.exclude_links_from_lockfile != exclude_links_from_lockfile
    })
}

/// See [`auto_install_peers_changed`].
#[must_use]
pub fn recorded_dedupe_peers(recorded: Option<&LockfileSettings>) -> bool {
    recorded.and_then(|settings| settings.dedupe_peers).unwrap_or(false)
}

/// See [`auto_install_peers_changed`].
#[must_use]
pub fn recorded_implicit_types_peers(recorded: Option<&LockfileSettings>) -> bool {
    recorded.and_then(|settings| settings.implicit_types_peers).unwrap_or(false)
}

/// See [`auto_install_peers_changed`].
#[must_use]
pub fn recorded_peers_suffix_max_length(recorded: Option<&LockfileSettings>) -> u64 {
    recorded
        .and_then(|settings| settings.peers_suffix_max_length)
        .unwrap_or(crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH)
}

/// See [`auto_install_peers_changed`].
#[must_use]
pub fn recorded_inject_workspace_packages(recorded: Option<&LockfileSettings>) -> bool {
    recorded.is_some_and(|settings| settings.inject_workspace_packages)
}
