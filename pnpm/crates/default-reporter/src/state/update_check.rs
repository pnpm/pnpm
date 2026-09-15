use super::standalone_install_command;

/// Where the running pnpm came from, which is what decides how to update
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PnpmInstallSource {
    Corepack,
    PnpmHome,
    Elsewhere,
}

pub(super) fn detect_install_source() -> PnpmInstallSource {
    if std::env::var_os("COREPACK_ROOT").is_some() {
        PnpmInstallSource::Corepack
    } else if std::env::var_os("PNPM_HOME").is_some_and(|home| !home.is_empty()) {
        PnpmInstallSource::PnpmHome
    } else {
        PnpmInstallSource::Elsewhere
    }
}

/// pnpm's `renderUpdateCommand`: the command that updates the pnpm the
/// user is running.
pub(super) fn update_command(source: PnpmInstallSource) -> String {
    match source {
        PnpmInstallSource::PnpmHome => "pnpm self-update".to_string(),
        // `self-update` replaces the pnpm that `PNPM_HOME` manages. Corepack
        // refuses it outright, and an install another package manager owns is
        // resolved from that manager's bin directory rather than pnpm's home,
        // so a self-update would land beside the executable in use instead of
        // replacing it. The installer is the command that updates either one.
        PnpmInstallSource::Corepack | PnpmInstallSource::Elsewhere => {
            standalone_install_command().to_string()
        }
    }
}

pub(super) fn is_strictly_newer(latest: &str, version: &str) -> bool {
    match (node_semver::Version::parse(latest), node_semver::Version::parse(version)) {
        (Ok(l), Ok(v)) => l > v,
        _ => false,
    }
}
