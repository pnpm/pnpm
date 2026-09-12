use crate::{ParsePkgNameSuffixError, ParsePkgVerPeerError, PkgNameSuffix, PkgVerPeer};
use pnpm_crypto_hash::shorten_virtual_store_name;

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
    /// 120; see `pnpm_modules_yaml::DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH`
    /// — referenced by name rather than as an intra-doc link because
    /// `pnpm-lockfile` deliberately does not depend on
    /// `pnpm-modules-yaml`).
    #[must_use]
    pub fn to_virtual_store_name(&self, max_length: usize) -> String {
        let escape_for_fs = |character: char| {
            matches!(character, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#')
        };
        let mut filename = self.to_string().replace(escape_for_fs, "+");
        if filename.contains('(') {
            if filename.ends_with(')') {
                filename.pop();
            }
            filename = filename.replace(")(", "_").replace(['(', ')'], "_");
        }
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

    /// The package id pnpm addresses this package by outside the
    /// lockfile: the store-index row key (`store_index_key` /
    /// `git_hosted_store_index_key` — referenced as plain text because
    /// `pnpm-lockfile` deliberately does not depend on
    /// `pnpm-store-dir`), the `packageId` of a `pnpm:progress`
    /// event, and the resolution id the git fetchers build their
    /// `allowBuild` dep path from.
    ///
    /// For a registry package this is the peer-stripped key itself
    /// (`name@version`). For a non-registry resolution — a URL tarball,
    /// a git-host archive, a `type: git` dependency — it is the bare
    /// resolution id, without the `name@` prefix the lockfile key
    /// carries. The store index is a contract shared with the
    /// TypeScript CLI, which keys those rows by the bare id.
    #[must_use]
    pub fn pkg_id(&self) -> String {
        let mut rendered = self.to_string();
        // `try_get_package_id` borrows a prefix of its input unless it
        // drops the `name@` prefix, so truncating reuses this allocation
        // rather than cloning the slice into a second one.
        let pkg_id_len = match pnpm_deps_path::try_get_package_id(&rendered) {
            std::borrow::Cow::Borrowed(pkg_id) => pkg_id.len(),
            std::borrow::Cow::Owned(pkg_id) => return pkg_id,
        };
        rendered.truncate(pkg_id_len);
        rendered
    }
}

#[cfg(test)]
mod tests;
