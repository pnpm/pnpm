/// How a resolved version is pinned into a manifest's version range when a
/// dependency is added or updated.
///
/// Mirrors pnpm's `PinnedVersion` string-literal union
/// (<https://github.com/pnpm/pnpm/blob/086c5e91e8/core/types/src/misc.ts#L71-L75>).
/// [`PinnedVersion::None`] is part of that union; the `--save-exact` /
/// `--save-prefix` interpreter in [`PinnedVersion::from_save_options`] never
/// produces it, matching pnpm's `getPinnedVersion`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PinnedVersion {
    /// Save with a caret range (`^version`), allowing same-major updates.
    /// pnpm's default.
    #[default]
    Major,
    /// Save with a tilde range (`~version`), allowing patch-level updates.
    Minor,
    /// Save the exact resolved version with no range operator.
    Patch,
    /// Equivalent to [`PinnedVersion::Major`] when turned into a range.
    None,
}

impl PinnedVersion {
    /// Interpret the `--save-exact` and `--save-prefix` flags into a
    /// [`PinnedVersion`].
    ///
    /// Mirrors pnpm's `getPinnedVersion`
    /// (<https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/commands/src/getPinnedVersion.ts>).
    #[must_use]
    pub fn from_save_options(save_exact: bool, save_prefix: Option<&str>) -> Self {
        if save_exact || save_prefix == Some("") {
            return PinnedVersion::Patch;
        }
        if save_prefix == Some("~") { PinnedVersion::Minor } else { PinnedVersion::Major }
    }

    /// The range operator prepended to the resolved version when serializing
    /// it into a manifest specifier.
    ///
    /// Mirrors pnpm's `createVersionSpecFromResolvedVersion`
    /// (<https://github.com/pnpm/pnpm/blob/086c5e91e8/pkg-manifest/utils/src/updateProjectManifestObject.ts#L29-L45>).
    #[must_use]
    pub fn range_prefix(self) -> &'static str {
        match self {
            PinnedVersion::Major | PinnedVersion::None => "^",
            PinnedVersion::Minor => "~",
            PinnedVersion::Patch => "",
        }
    }
}

#[cfg(test)]
mod tests;
