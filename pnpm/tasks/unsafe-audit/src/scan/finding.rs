//! What the scan found, and how a departure from the policy reads.

use std::path::PathBuf;

/// What an `unsafe` the scan found is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An `unsafe extern` block, which declares a foreign ABI. Only a
    /// `-sys` crate may write one, and it discharges no obligation,
    /// because it states signatures rather than calling anything.
    ForeignAbiBlock,
    /// An `unsafe extern "abi" fn` signature: a callback the operating
    /// system calls, or a type naming one. It discharges no obligation
    /// either, for the same reason.
    ForeignSignature,
    /// Everything else: a block, function, implementation or trait whose
    /// obligation a safety comment has to discharge.
    Operation,
}

/// One `unsafe` the scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// The file it is in, relative to the workspace root.
    pub file: PathBuf,
    /// Its line, counting from one.
    pub line: usize,
    /// What the line reads, trimmed.
    pub text: String,
    /// What it is.
    pub kind: Kind,
    /// Whether a safety comment precedes it.
    pub has_safety_comment: bool,
}

/// Something the policy forbids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// A crate the policy does not exempt holds `unsafe`.
    UnsafeInConsumer { crate_name: String, occurrence: Occurrence },
    /// The workspace manifest does not lint `unsafe_code`, so nothing
    /// stops an `unsafe` compiling in a crate that never asked to hold one.
    UnsafeCodeNotLinted { level: Option<String> },
    /// A member does not take the workspace lint table, so the
    /// `unsafe_code` lint never reaches it.
    NotInheritingWorkspaceLints { crate_name: String },
    /// A crate the policy exempts does not opt out of the lint, so the
    /// `unsafe` it exists to hold is a warning.
    MissingOptOut { crate_name: String, root: PathBuf },
    /// A crate the policy does not exempt opts out of the lint.
    UnsanctionedOptOut { crate_name: String, root: PathBuf },
    /// A crate is exempt but holds no `unsafe`, so the exemption is stale.
    StaleExemption { crate_name: String },
    /// An `unsafe extern` block appears outside a `-sys` crate.
    ForeignAbiOutsideSysCrate { crate_name: String, occurrence: Occurrence },
    /// An `unsafe` is not preceded by a safety comment.
    MissingSafetyComment { crate_name: String, occurrence: Occurrence },
}

impl std::fmt::Display for Occurrence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}: {}", self.file.display(), self.line, self.text)
    }
}

impl std::fmt::Display for Finding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.statement())?;
        match self.remedy() {
            Some(remedy) => write!(formatter, "\n    {remedy}"),
            None => Ok(()),
        }
    }
}

impl Finding {
    /// What the departure is.
    fn statement(&self) -> String {
        match self {
            Self::UnsafeInConsumer { crate_name, occurrence } => format!(
                "{crate_name} is not exempt from the unsafe policy, but holds `unsafe` at \
                 {occurrence}",
            ),
            Self::UnsafeCodeNotLinted { level: Some(level) } => format!(
                "the workspace manifest lints `unsafe_code` at `{level}`, which does not hold \
                 a crate to the policy.",
            ),
            Self::UnsafeCodeNotLinted { level: None } => {
                "the workspace manifest does not lint `unsafe_code`, so nothing stops an \
                 `unsafe` compiling."
                    .to_owned()
            }
            Self::NotInheritingWorkspaceLints { crate_name } => format!(
                "{crate_name} does not take the workspace lint table, so the `unsafe_code` \
                 lint does not reach it.",
            ),
            Self::MissingOptOut { crate_name, root } => format!(
                "{crate_name} is exempt from the unsafe policy, so {} needs an \
                 `allow(unsafe_code)` inner attribute.",
                root.display(),
            ),
            Self::UnsanctionedOptOut { crate_name, root } => format!(
                "{crate_name} is not exempt from the unsafe policy, but {} opts out of the \
                 `unsafe_code` lint.",
                root.display(),
            ),
            Self::StaleExemption { crate_name } => {
                format!("{crate_name} is exempt from the unsafe policy but holds no `unsafe`.")
            }
            Self::ForeignAbiOutsideSysCrate { crate_name, occurrence } => format!(
                "{crate_name} is not a `-sys` crate, but declares a foreign ABI at {occurrence}",
            ),
            Self::MissingSafetyComment { crate_name, occurrence } => format!(
                "{crate_name} has an `unsafe` with no `// SAFETY:` comment above it, at {occurrence}",
            ),
        }
    }

    /// What to do about it, where the statement does not already say.
    fn remedy(&self) -> Option<&'static str> {
        match self {
            Self::UnsafeInConsumer { .. } => Some(
                "Move the call into the abstraction crate that owns the surface, or add a new \
                 one for a surface that has no owner yet.",
            ),
            Self::UnsafeCodeNotLinted { .. } => Some(
                "`[workspace.lints.rust]` needs at least `unsafe_code = \"warn\"`; CI's \
                 `--deny warnings` makes it an error.",
            ),
            Self::NotInheritingWorkspaceLints { .. } => {
                Some("Its manifest needs a `[lints]` table with `workspace = true`.")
            }
            Self::MissingOptOut { .. } => Some(
                "Without it the `unsafe` the crate exists to hold is a warning, and CI's \
                 `--deny warnings` an error.",
            ),
            Self::UnsanctionedOptOut { .. } => Some(
                "Drop the attribute, or add the crate to `exempt()` if the policy really does \
                 let it hold `unsafe`.",
            ),
            Self::StaleExemption { .. } => {
                Some("Drop its entry from `exempt()`, and the opt-out from its crate root.")
            }
            Self::ForeignAbiOutsideSysCrate { .. } | Self::MissingSafetyComment { .. } => None,
        }
    }
}
