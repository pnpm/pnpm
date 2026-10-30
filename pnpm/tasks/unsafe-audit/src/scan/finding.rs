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
#[derive(Debug, derive_more::Display, Clone, PartialEq, Eq)]
#[display("{}:{line}: {text}", file.display())]
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
///
/// A variant's `Display` is what the audit prints for it: what the
/// departure is, and on an indented line below, what to do about it where
/// the first line does not already say.
#[derive(Debug, derive_more::Display, Clone, PartialEq, Eq)]
pub enum Finding {
    /// A crate the policy does not exempt holds `unsafe`.
    #[display(
        "{crate_name} is not exempt from the unsafe policy, but holds `unsafe` at {occurrence}\n    \
         Move the call into the abstraction crate that owns the surface, or add a new one for a \
         surface that has no owner yet."
    )]
    UnsafeInConsumer { crate_name: String, occurrence: Occurrence },
    /// The workspace manifest lints `unsafe_code`, but at a level that
    /// lets one compile.
    #[display(
        "the workspace manifest lints `unsafe_code` at `{level}`, which does not hold a crate to \
         the policy.\n    \
         `[workspace.lints.rust]` needs at least `unsafe_code = \"warn\"`; CI's `--deny warnings` \
         makes it an error."
    )]
    UnsafeCodeLintedTooWeakly { level: String },
    /// The workspace manifest does not lint `unsafe_code`, so nothing
    /// stops an `unsafe` compiling in a crate that never asked to hold one.
    #[display(
        "the workspace manifest does not lint `unsafe_code`, so nothing stops an `unsafe` \
         compiling.\n    \
         `[workspace.lints.rust]` needs at least `unsafe_code = \"warn\"`; CI's `--deny warnings` \
         makes it an error."
    )]
    UnsafeCodeNotLinted,
    /// A member does not take the workspace lint table, so the
    /// `unsafe_code` lint never reaches it.
    #[display(
        "{crate_name} does not take the workspace lint table, so the `unsafe_code` lint does not \
         reach it.\n    \
         Its manifest needs a `[lints]` table with `workspace = true`."
    )]
    NotInheritingWorkspaceLints { crate_name: String },
    /// A crate the policy exempts does not opt out of the lint, so the
    /// `unsafe` it exists to hold is a warning.
    #[display(
        "{crate_name} is exempt from the unsafe policy, so {} needs an `allow(unsafe_code)` inner \
         attribute.\n    \
         Without it the `unsafe` the crate exists to hold is a warning, and CI's `--deny warnings` \
         an error.",
        root.display(),
    )]
    MissingOptOut { crate_name: String, root: PathBuf },
    /// A crate the policy does not exempt opts out of the lint.
    #[display(
        "{crate_name} is not exempt from the unsafe policy, but {} opts out of the `unsafe_code` \
         lint.\n    \
         Drop the attribute, or add the crate to `exempt()` if the policy really does let it hold \
         `unsafe`.",
        root.display(),
    )]
    UnsanctionedOptOut { crate_name: String, root: PathBuf },
    /// A crate is exempt but holds no `unsafe`, so the exemption is stale.
    #[display(
        "{crate_name} is exempt from the unsafe policy but holds no `unsafe`.\n    \
         Drop its entry from `exempt()`, and the opt-out from its crate root."
    )]
    StaleExemption { crate_name: String },
    /// An `unsafe extern` block appears outside a `-sys` crate.
    #[display("{crate_name} is not a `-sys` crate, but declares a foreign ABI at {occurrence}")]
    ForeignAbiOutsideSysCrate { crate_name: String, occurrence: Occurrence },
    /// An `unsafe` is not preceded by a safety comment.
    #[display(
        "{crate_name} has an `unsafe` with no `// SAFETY:` comment above it, at {occurrence}"
    )]
    MissingSafetyComment { crate_name: String, occurrence: Occurrence },
}
