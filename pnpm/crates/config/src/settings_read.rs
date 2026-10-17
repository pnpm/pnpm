//! Which settings a load reads from the global `config.yaml` and
//! `pnpm-workspace.yaml`.

/// The settings that decide whether pnpm switches to the version a project
/// pins, or runs the command itself.
///
/// - `pmOnFail` and `runtimeOnFail` override what the manifest's pins ask
///   for.
/// - `lockfile` and `lockfileDir` say whether and where the env lockfile
///   records the pinned version, and `globalDir` where a pin the lockfile
///   does not record keeps it.
/// - `frozenLockfile` refuses to record a version the lockfile does not.
/// - `packages` says whether the project belongs to the workspace whose root
///   holds the pin.
/// - `ci`, `color`, `loglevel`, and `reporter` shape the warnings the checks
///   raise.
///
/// A setting outside this list cannot steer the decision, and one this pnpm
/// cannot read cannot stop it.
pub const PIN_DECISION_SETTINGS: &[&str] = &[
    "ci",
    "color",
    "frozenLockfile",
    "globalDir",
    "lockfile",
    "lockfileDir",
    "loglevel",
    "packages",
    "pmOnFail",
    "reporter",
    "runtimeOnFail",
];

/// Which settings a load reads from the global `config.yaml` and
/// `pnpm-workspace.yaml`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SettingsRead {
    /// Every setting. One this pnpm cannot read fails the load.
    #[default]
    All,
    /// Every setting this pnpm can read. Each top-level setting that does
    /// not parse or validate on its own is left out, and so is any source
    /// that still fails, such as `_auth` or a project `.npmrc`
    /// `tokenHelper`. Every other source still applies, so a repository that
    /// breaks its own files cannot drop the machine's settings. A newer pnpm
    /// may give a setting a shape or a value this one rejects, so the load
    /// that switches to the pinned pnpm reads this way.
    Readable,
    /// Only the [`PIN_DECISION_SETTINGS`] this pnpm can read. No `.npmrc` or
    /// auth source is read.
    PinDecision,
}

impl SettingsRead {
    /// Whether a setting or source this pnpm cannot read is left out rather
    /// than failing the load.
    #[must_use]
    pub fn skips_unreadable(self) -> bool {
        self != Self::All
    }
}
