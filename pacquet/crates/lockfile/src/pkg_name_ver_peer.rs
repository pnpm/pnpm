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
    /// `max_length` is `Modules.virtual_store_dir_max_length` (default
    /// 120; see `pacquet_modules_yaml::DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH`
    /// — referenced by name rather than as an intra-doc link because
    /// `pacquet-lockfile` deliberately does not depend on
    /// `pacquet-modules-yaml`).
    #[must_use]
    pub fn to_virtual_store_name(&self, max_length: usize) -> String {
        let escape_for_fs = |character: char| {
            matches!(character, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#')
        };
        let filename = self
            .to_string()
            .replace(escape_for_fs, "+")
            .replace(")(", "_")
            .replace('(', "_")
            .replace(')', "");
        shorten_virtual_store_name(filename, max_length)
    }

    /// Return a new [`PkgNameVerPeer`] with the peer-dependency suffix stripped.
    ///
    /// This converts a v9 snapshot key into the corresponding `packages:` key,
    /// which identifies the package version independent of peer context.
    #[must_use]
    pub fn without_peer(&self) -> PkgNameVerPeer {
        PkgNameVerPeer::new(self.name.clone(), self.suffix.without_peer())
    }
}

#[cfg(test)]
mod tests;
