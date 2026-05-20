use crate::{ParsePkgNameSuffixError, ParsePkgVerPeerError, PkgNameSuffix, PkgVerPeer};
use pacquet_crypto_hash::shorten_virtual_store_name;

/// Syntax: `{name}@{version}({peers})`
///
/// Example: `react-json-view@1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)`
///
/// **NOTE:** The suffix isn't guaranteed to be correct. It is only assumed to be.
pub type PkgNameVerPeer = PkgNameSuffix<PkgVerPeer>;

/// Error when parsing [`PkgNameVerPeer`] from a string.
pub type ParsePkgNameVerPeerError = ParsePkgNameSuffixError<ParsePkgVerPeerError>;

impl PkgNameVerPeer {
    /// Construct the name of the corresponding subdirectory in the
    /// virtual store directory. When the resulting name would exceed
    /// `max_length` bytes, fall back to a hash-shortened form so the
    /// path stays within filesystem limits.
    ///
    /// Mirrors upstream's
    /// [`depPathToFilename`](https://github.com/pnpm/pnpm/blob/1819226b51/deps/path/src/index.ts#L169-L180):
    /// the lossy escape (parens → underscores, `/` → `+`) runs first,
    /// then if the filename is longer than `max_length` *or* contains
    /// uppercase characters (the case-insensitive-filesystem guard),
    /// the result becomes `<filename truncated to max_length - 33>_<32-hex-sha256>`.
    /// The `file+` prefix skips the case guard so file-protocol deps
    /// don't all hash-shorten just because their on-disk paths happen
    /// to contain capitals.
    ///
    /// `max_length` is `Modules.virtual_store_dir_max_length` (default
    /// 120; see `pacquet_modules_yaml::DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH`
    /// — referenced by name rather than as an intra-doc link because
    /// `pacquet-lockfile` deliberately does not depend on
    /// `pacquet-modules-yaml`).
    pub fn to_virtual_store_name(&self, max_length: usize) -> String {
        let filename = self
            .to_string()
            .replace('/', "+")
            .replace(")(", "_")
            .replace('(', "_")
            .replace(')', "");
        shorten_virtual_store_name(filename, max_length)
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
            .expect("a prefix + the displayed version is always a valid PkgVerPeer");
        PkgNameVerPeer::new(self.name.clone(), bare)
    }
}

#[cfg(test)]
mod tests;
