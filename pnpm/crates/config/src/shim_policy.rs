use super::{Deserialize, Serialize};

/// The resolved per-package policy in `globalShims`.
///
/// `Auto` (the record value `"auto"`, or its shorthand `true`) defers
/// to artifact authentication:
/// publisher-signature-verified candidates run without prompting, all
/// others go through the candidate-bound trust prompt. `Prompt` always
/// asks, even for authenticated candidates. `Always` always switches
/// without asking — the user pre-answered the prompt in machine-local
/// configuration, which a project cannot write to. `Off` (the record
/// value `false`) disables the package's context-aware shim entirely.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ShimPolicy {
    #[default]
    Off,
    Auto,
    Prompt,
    Always,
}

/// One value of the `globalShims` record: a named policy (`"auto"`,
/// `"prompt"`, `"always"`) or the boolean shorthands (`true` ≡
/// `"auto"`, `false` ≡ disabled). See [`ShimPolicy`] for the semantics
/// each maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShimPolicyValue {
    Toggle(bool),
    Named(NamedShimPolicy),
}

impl ShimPolicy {
    /// The `globalShims` value this policy is written as.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ShimPolicy::Off => "off",
            ShimPolicy::Auto => "auto",
            ShimPolicy::Prompt => "prompt",
            ShimPolicy::Always => "always",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NamedShimPolicy {
    Auto,
    Prompt,
    Always,
}

impl ShimPolicyValue {
    /// Whether a package recorded with this value dispatches at all.
    ///
    /// A recorded value that does not is the user switching a shim off —
    /// including one of the built-in defaults, which is the only way to
    /// switch those off. Clearing such an entry turns the shim back on.
    #[must_use]
    pub fn dispatches(self) -> bool {
        self.resolve() != ShimPolicy::Off
    }

    fn resolve(self) -> ShimPolicy {
        match self {
            ShimPolicyValue::Toggle(false) => ShimPolicy::Off,
            ShimPolicyValue::Toggle(true) | ShimPolicyValue::Named(NamedShimPolicy::Auto) => {
                ShimPolicy::Auto
            }
            ShimPolicyValue::Named(NamedShimPolicy::Prompt) => ShimPolicy::Prompt,
            ShimPolicyValue::Named(NamedShimPolicy::Always) => ShimPolicy::Always,
        }
    }
}

/// One configuration layer of the `globalShims` setting:
/// either a record of package names to policy values, or a scalar
/// shorthand. Layers fold into the resolved [`GlobalShims`]
/// via [`GlobalShims::apply`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GlobalShimsSetting {
    /// `globalShims: false` disables every context-aware
    /// shim; `true` resets to the built-in defaults.
    Toggle(bool),
    /// `globalShims: { <package>: <policy> }` merges
    /// key-wise over the defaults and lower layers, so one `bun: false`
    /// entry disables a single default without restating the rest.
    Entries(std::collections::HashMap<String, ShimPolicyValue>),
}

/// The resolved `globalShims` setting: which globally
/// installed packages get context-aware shims and under which trust
/// policy, keyed by the providing package's manifest name (so an entry
/// for `typescript` covers its `tsc` bin).
///
/// The built-in default enables the managed runtimes — `node`, `deno`,
/// and `bun` — with the [`ShimPolicy::Auto`] policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalShims {
    pub(super) entries: std::collections::HashMap<String, ShimPolicy>,
}

impl Default for GlobalShims {
    fn default() -> Self {
        Self {
            entries: ["node", "deno", "bun"]
                .into_iter()
                .map(|name| (name.to_string(), ShimPolicy::Auto))
                .collect(),
        }
    }
}

impl GlobalShims {
    /// Fold one configuration layer into the resolved setting. Records
    /// merge key-wise; the scalar shorthands replace the accumulated
    /// state (`false` → nothing dispatches, `true` → the defaults).
    pub fn apply(&mut self, layer: &GlobalShimsSetting) {
        match layer {
            GlobalShimsSetting::Toggle(false) => self.entries.clear(),
            GlobalShimsSetting::Toggle(true) => *self = Self::default(),
            GlobalShimsSetting::Entries(entries) => {
                for (name, value) in entries {
                    self.entries.insert(name.clone(), value.resolve());
                }
            }
        }
    }

    /// Every package with an entry, and the policy it resolved to.
    pub fn entries(&self) -> impl Iterator<Item = (&str, ShimPolicy)> {
        self.entries.iter().map(|(name, policy)| (name.as_str(), *policy))
    }

    #[must_use]
    pub fn policy(&self, package_name: &str) -> ShimPolicy {
        self.entries.get(package_name).copied().unwrap_or(ShimPolicy::Off)
    }

    #[must_use]
    pub fn is_enabled(&self, package_name: &str) -> bool {
        self.policy(package_name) != ShimPolicy::Off
    }

    /// Whether no package is eligible at all — the dispatcher's cheap
    /// early exit.
    #[must_use]
    pub fn dispatches_nothing(&self) -> bool {
        self.entries.values().all(|policy| *policy == ShimPolicy::Off)
    }
}
