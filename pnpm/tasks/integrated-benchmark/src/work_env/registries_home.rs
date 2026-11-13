//! Where the `pnpr` of a compared revision reads its registries from.

use std::path::{Path, PathBuf};

/// The loopback networks the benchmark's servers listen on, for
/// `routes.allowedPrivateNetworks`.
pub(super) const LOOPBACK_NETWORKS: &[&str] = &["127.0.0.0/8", "::1"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RegistriesHome {
    /// The config file declares `registries`, `defaultRegistry`, and
    /// `routes.public`. The resolver may reach any port of the server's own
    /// host.
    ConfigFile,
    /// The metadata database holds them, stored with `pnpr registries import`
    /// before the server starts. No upstream may be on the server's own
    /// `public_url` host, the resolver reaches that host only at the
    /// `public_url` origin, and loopback addresses need
    /// `routes.allowedPrivateNetworks`.
    Database,
}

impl RegistriesHome {
    /// Read from the checkout at `source`: one whose registry-mock fixtures
    /// keep the registries in a document of their own stores them in the
    /// database.
    pub(super) fn of_source(source: &Path) -> Self {
        if fixture_registries(source).is_some() { Self::Database } else { Self::ConfigFile }
    }
}

/// The registry-mock registries document of the checkout at `source`, when
/// it has one.
pub(super) fn fixture_registries(source: &Path) -> Option<PathBuf> {
    Some(source.join("pnpr/.fixtures/registries.yaml")).filter(|path| path.is_file())
}

/// The checkout a `target/release/pnpr` binary was built from.
pub(super) fn revision_source(binary: &Path) -> &Path {
    binary
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("revision binary is under target/release")
}
