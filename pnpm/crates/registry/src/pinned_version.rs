/// How a resolved version is pinned into a manifest's version range when a
/// dependency is added or updated.
///
/// [`PinnedVersion::None`] is a valid variant, but the `--save-exact` /
/// `--save-prefix` interpreter in [`PinnedVersion::from_save_options`] never
/// produces it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PinnedVersion {
    /// Save with a caret range (`^version`), allowing same-major updates.
    /// pnpm's default.
    #[default]
    Major,
    /// Preserve an explicit equals exact range (`=version`).
    Exact,
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
    #[must_use]
    pub fn from_save_options(save_exact: bool, save_prefix: Option<&str>) -> Self {
        if save_exact || save_prefix == Some("") {
            return PinnedVersion::Patch;
        }
        if save_prefix == Some("~") { PinnedVersion::Minor } else { PinnedVersion::Major }
    }

    /// The range operator prepended to the resolved version when serializing
    /// it into a manifest specifier.
    #[must_use]
    pub fn range_prefix(self) -> &'static str {
        match self {
            PinnedVersion::Major | PinnedVersion::None => "^",
            PinnedVersion::Exact => "=",
            PinnedVersion::Minor => "~",
            PinnedVersion::Patch => "",
        }
    }
}

#[cfg(test)]
mod tests;
