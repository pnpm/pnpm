/// How a resolved version is written back into a manifest specifier when a
/// dependency is added or updated: which range operator it is saved with.
///
/// [`RangeSpecStyle::None`] is a valid variant, but the `--save-exact` /
/// `--save-prefix` interpreter in [`RangeSpecStyle::from_save_options`] never
/// produces it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RangeSpecStyle {
    /// Save with a caret range (`^version`), allowing same-major updates.
    /// pnpm's default.
    #[default]
    Major,
    /// Save with a tilde range (`~version`), allowing patch-level updates.
    Minor,
    /// Save the exact resolved version with no range operator.
    Patch,
    /// Save the exact resolved version with a leading `=` operator,
    /// preserving an explicit `=` pin from the previous specifier or a
    /// `--save-prefix` of `=`. Selects
    /// the same single-version range as [`RangeSpecStyle::Patch`]; only the
    /// written operator differs.
    Exact,
    /// Equivalent to [`RangeSpecStyle::Major`] when turned into a range.
    None,
}

impl RangeSpecStyle {
    /// Interpret the `--save-exact` and `--save-prefix` flags into a
    /// [`RangeSpecStyle`]. `--save-exact` (like the empty `--save-prefix`)
    /// wins and saves the bare version; a `--save-prefix` of `=` saves the
    /// version with an explicit `=` operator.
    #[must_use]
    pub fn from_save_options(save_exact: bool, save_prefix: Option<&str>) -> Self {
        if save_exact || save_prefix == Some("") {
            return RangeSpecStyle::Patch;
        }
        match save_prefix {
            Some("=") => RangeSpecStyle::Exact,
            Some("~") => RangeSpecStyle::Minor,
            _ => RangeSpecStyle::Major,
        }
    }

    /// The range operator prepended to the resolved version when serializing
    /// it into a manifest specifier.
    #[must_use]
    pub fn range_prefix(self) -> &'static str {
        match self {
            RangeSpecStyle::Major | RangeSpecStyle::None => "^",
            RangeSpecStyle::Minor => "~",
            RangeSpecStyle::Patch => "",
            RangeSpecStyle::Exact => "=",
        }
    }

    /// The width of the range the style selects, with the spelling
    /// distinction collapsed away: [`RangeSpecStyle::Exact`] pins the same
    /// range as [`RangeSpecStyle::Patch`]. Match on this instead of the raw
    /// variants when only the range width matters.
    #[must_use]
    pub fn granularity(self) -> RangeSpecGranularity {
        match self {
            RangeSpecStyle::Major => RangeSpecGranularity::Major,
            RangeSpecStyle::Minor => RangeSpecGranularity::Minor,
            RangeSpecStyle::Patch | RangeSpecStyle::Exact => RangeSpecGranularity::Patch,
            RangeSpecStyle::None => RangeSpecGranularity::None,
        }
    }
}

/// The width of the semver range a [`RangeSpecStyle`] selects (see
/// [`RangeSpecStyle::granularity`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeSpecGranularity {
    Major,
    Minor,
    Patch,
    None,
}

#[cfg(test)]
mod tests;
