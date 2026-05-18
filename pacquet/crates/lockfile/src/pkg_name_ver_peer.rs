use crate::{ParsePkgNameSuffixError, ParsePkgVerPeerError, PkgNameSuffix, PkgVerPeer};

/// Syntax: `{name}@{version}({peers})`
///
/// Example: `react-json-view@1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)`
///
/// **NOTE:** The suffix isn't guaranteed to be correct. It is only assumed to be.
pub type PkgNameVerPeer = PkgNameSuffix<PkgVerPeer>;

/// Error when parsing [`PkgNameVerPeer`] from a string.
pub type ParsePkgNameVerPeerError = ParsePkgNameSuffixError<ParsePkgVerPeerError>;

impl PkgNameVerPeer {
    /// Construct the name of the corresponding subdirectory in the virtual store directory.
    pub fn to_virtual_store_name(&self) -> String {
        // the code below is far from optimal,
        // optimization requires parser combinator
        self.to_string().replace('/', "+").replace(")(", "_").replace('(', "_").replace(')', "")
    }

    /// Return a new [`PkgNameVerPeer`] with the peer-dependency suffix stripped.
    ///
    /// This converts a v9 snapshot key (e.g. `react-dom@17.0.2(react@17.0.2)`)
    /// into the corresponding `packages:` key (e.g. `react-dom@17.0.2`), which
    /// identifies the package version independent of peer context. The
    /// scheme prefix (e.g. `runtime:` for pnpm v11 runtime entries) is
    /// preserved so a runtime snapshot key like
    /// `node@runtime:22.0.0(some@peer)` resolves to the matching
    /// `packages:` entry `node@runtime:22.0.0` rather than the
    /// non-existent `node@22.0.0`.
    pub fn without_peer(&self) -> PkgNameVerPeer {
        let prefix = self.suffix.prefix();
        let bare_input = format!("{}{}", prefix, self.suffix.version());
        let bare = bare_input
            .parse::<PkgVerPeer>()
            .expect("a prefix + bare semver version is always a valid PkgVerPeer");
        PkgNameVerPeer::new(self.name.clone(), bare)
    }
}

#[cfg(test)]
mod tests;
