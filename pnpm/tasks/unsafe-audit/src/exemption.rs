//! Which crates may hold `unsafe`, and why each one may.

/// Which tier a crate that may hold `unsafe` is in.
///
/// The tiers are the policy's; what distinguishes them is what the crate
/// is allowed to contain, which is why the audit needs to know which one a
/// crate is in rather than just that it is exempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Declares a foreign ABI and nothing else. The only tier that may
    /// write an `unsafe extern` block.
    ForeignAbi,
    /// Turns one foreign or raw-pointer API into a safe one. Its `unsafe`
    /// is its purpose, and its public API exposes no way to misuse it.
    Abstraction,
    /// Test support: fixtures and harnesses that need a foreign call to
    /// set up a scenario production code never creates.
    TestSupport,
    /// A macro the crate uses expands to `unsafe`. The crate writes none
    /// of its own, which is what the audit holds it to: every `unsafe` its
    /// source spells out is hand-written, and so a finding.
    MacroGenerated,
}

/// A crate the policy lets hold `unsafe`, and the reason it does.
#[derive(Debug, Clone, Copy)]
pub struct Exemption {
    /// The crate's package name.
    pub crate_name: &'static str,
    /// Which tier it is in.
    pub tier: Tier,
    /// What it abstracts, in one clause. Not a formality: an exemption
    /// whose reason cannot be written in one clause is a crate holding
    /// more than one concern.
    pub reason: &'static str,
}

/// The crates the policy lets hold `unsafe`.
///
/// Every other workspace member takes the workspace's `unsafe_code` lint
/// and holds no `unsafe`. Adding to this list is a policy decision, not a
/// convenience: read
/// [the policy](../../../UNSAFE_POLICY.md) before you do.
#[must_use]
pub fn exempt() -> &'static [Exemption] {
    EXEMPT
}

const EXEMPT: &[Exemption] = &[
    Exemption {
        crate_name: "pnpm-webcontainer-sys",
        tier: Tier::ForeignAbi,
        reason: "declares the WebContainer supervisor's imports",
    },
    Exemption {
        crate_name: "pnpm-ffi-fs",
        tier: Tier::Abstraction,
        reason: "the filesystem foreign calls pnpm makes",
    },
    Exemption {
        crate_name: "pnpm-ffi-process",
        tier: Tier::Abstraction,
        reason: "the process-control foreign calls pnpm makes",
    },
    Exemption {
        crate_name: "pnpm-ffi-security",
        tier: Tier::Abstraction,
        reason: "the Apple Security.framework calls pnpm makes",
    },
    Exemption {
        crate_name: "pnpm-ffi-signal",
        tier: Tier::Abstraction,
        reason: "the signal and console-event foreign calls pnpm makes",
    },
    Exemption {
        crate_name: "pnpm-ffi-sqlite",
        tier: Tier::Abstraction,
        reason: "the SQLite build-configuration query pnpm makes",
    },
    Exemption {
        crate_name: "pnpm-ffi-terminal",
        tier: Tier::Abstraction,
        reason: "the terminal and console foreign calls pnpm makes",
    },
    Exemption {
        crate_name: "pnpm-ffi-webcontainer",
        tier: Tier::Abstraction,
        reason: "the WebContainer supervisor's imports",
    },
    Exemption {
        crate_name: "pnpm-signal-registry",
        tier: Tier::Abstraction,
        reason: "the registries a signal handler walks without allocating",
    },
    Exemption {
        crate_name: "pnpm-napi",
        tier: Tier::MacroGenerated,
        reason: "the `napi` macro expands to the Node addon's bindings",
    },
    Exemption {
        crate_name: "pnpm-testing-utils",
        tier: Tier::TestSupport,
        reason: "the harnesses tests run under",
    },
];

/// The exemption for `crate_name`, if it has one.
#[must_use]
pub fn exemption_for(crate_name: &str) -> Option<Exemption> {
    exempt()
        .iter()
        .copied()
        .find(|exemption| exemption.crate_name == crate_name)
}

#[cfg(test)]
mod tests;
