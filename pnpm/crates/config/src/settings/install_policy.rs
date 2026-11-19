use super::Config;

/// What `--force`, `--reinstall`, and `--ignore-platform-checks` make an
/// install do. Install gates read these rather than the fields.
impl Config {
    /// Whether this install materializes every snapshot the lockfile
    /// names, `cpu` / `os` / `libc` / `engines` notwithstanding:
    /// [`ignore_platform_checks`](Self::ignore_platform_checks), or `--force`
    /// under [`force_ignores_platform`](Self::force_ignores_platform).
    /// Every installability gate reads this rather than `force` alone.
    pub fn installs_incompatible_packages(&self) -> bool {
        self.ignore_platform_checks || (self.force && self.force_ignores_platform)
    }

    /// Whether this install re-materializes every slot, changed or not:
    /// [`reinstall`](Self::reinstall) or [`force`](Self::force). Every
    /// slot-reuse gate reads this rather than either field alone.
    pub fn reinstalls_every_package(&self) -> bool {
        self.reinstall || self.force
    }

    /// Whether this install has to evaluate every snapshot again, so
    /// neither the skip set nor the up-to-date verdict an earlier install
    /// recorded applies to it.
    pub fn reevaluates_every_snapshot(&self) -> bool {
        self.reinstalls_every_package() || self.installs_incompatible_packages()
    }

    /// [`engine_strict`](Self::engine_strict) as an install applies it.
    /// `--force` and `--ignore-platform-checks` lift it, as npm's `--force`
    /// does: an `engines` mismatch on a required package warns instead of
    /// failing the install.
    pub fn effective_engine_strict(&self) -> bool {
        self.engine_strict && !self.force && !self.ignore_platform_checks
    }
}
