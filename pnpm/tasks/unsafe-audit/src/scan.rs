//! Reading the policy off the source.

use crate::{Exemption, Member, Tier, exemption::exemption_for};
use std::path::{Path, PathBuf};

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

/// Every way the workspace rooted at `root` departs from the policy.
///
/// An empty answer is the policy holding.
#[must_use]
pub fn audit(root: &Path) -> Vec<Finding> {
    let mut findings = workspace_findings(root);
    findings.extend(crate::members(root).iter().flat_map(|member| audit_member(root, member)));
    findings
}

/// The check that is the workspace's rather than any member's: a member
/// inheriting the lint table means nothing unless the table lints
/// `unsafe_code`.
fn workspace_findings(root: &Path) -> Vec<Finding> {
    let level = crate::crates::workspace_unsafe_code_level(root);
    let linted = level
        .as_deref()
        .is_some_and(|level| matches!(level, "warn" | "deny" | "forbid"));
    if linted { Vec::new() } else { vec![Finding::UnsafeCodeNotLinted { level }] }
}

fn audit_member(root: &Path, member: &Member) -> Vec<Finding> {
    let mut findings = Vec::new();
    if !member.inherits_workspace_lints {
        findings.push(Finding::NotInheritingWorkspaceLints { crate_name: member.name.clone() });
    }
    let occurrences: Vec<Occurrence> = member.sources
        .iter()
        .flat_map(|source| occurrences(root, source))
        .collect();
    match exemption_for(&member.name) {
        None => findings.extend(consumer_findings(root, member, occurrences)),
        Some(exemption) if exemption.tier == Tier::MacroGenerated => {
            findings.extend(macro_generated_findings(root, member, &occurrences));
        }
        Some(exemption) => findings.extend(exempt_findings(root, member, exemption, occurrences)),
    }
    findings
}

/// A crate the policy exempts holds the `unsafe` its exemption describes,
/// opts out of the lint so that `unsafe` compiles, and discharges each one
/// in a safety comment.
fn exempt_findings(
    root: &Path,
    member: &Member,
    exemption: Exemption,
    occurrences: Vec<Occurrence>,
) -> Vec<Finding> {
    if occurrences.is_empty() {
        return vec![Finding::StaleExemption { crate_name: member.name.clone() }];
    }
    let mut findings = missing_opt_outs(root, member);
    findings.extend(
        occurrences
            .into_iter()
            .filter_map(|occurrence| match occurrence.kind {
                Kind::ForeignAbiBlock => (exemption.tier != Tier::ForeignAbi).then(|| {
                    Finding::ForeignAbiOutsideSysCrate {
                        crate_name: member.name.clone(),
                        occurrence,
                    }
                }),
                Kind::ForeignSignature => None,
                Kind::Operation => (!occurrence.has_safety_comment).then(|| {
                    Finding::MissingSafetyComment { crate_name: member.name.clone(), occurrence }
                }),
            }),
    );
    findings
}

/// The crate roots of an exempt crate that do not opt out of the lint.
///
/// Every root, not just the one holding the `unsafe`: each is its own
/// compilation unit, and an opt-out on a root that turns out not to need
/// one costs nothing.
fn missing_opt_outs(root: &Path, member: &Member) -> Vec<Finding> {
    member.roots
        .iter()
        .filter(|crate_root| !opts_out_of_unsafe_code(crate_root))
        .map(|missing| Finding::MissingOptOut {
            crate_name: member.name.clone(),
            root: relative(root, missing),
        })
        .collect()
}

/// A crate the policy does not exempt may hold no `unsafe` at all, and may
/// not opt out of the lint that says so.
///
/// The `unsafe` scan is what enforces the policy here, and it reads the
/// source rather than the lint level, so an `unsafe` hidden behind a
/// hand-written `allow` is still a finding. Flagging the opt-out as well
/// catches the crate that is set up to hold one before it does.
fn consumer_findings(root: &Path, member: &Member, occurrences: Vec<Occurrence>) -> Vec<Finding> {
    let mut findings: Vec<Finding> = occurrences
        .into_iter()
        .map(|occurrence| Finding::UnsafeInConsumer { crate_name: member.name.clone(), occurrence })
        .collect();
    findings.extend(
        member.roots
            .iter()
            .filter(|crate_root| opts_out_of_unsafe_code(crate_root))
            .map(|opted_out| Finding::UnsanctionedOptOut {
                crate_name: member.name.clone(),
                root: relative(root, opted_out),
            }),
    );
    findings
}

/// A crate whose macros expand to `unsafe` writes none of its own, so the
/// scan's job there is the opposite: every occurrence the source holds is
/// hand-written, and so a finding.
fn macro_generated_findings(
    root: &Path,
    member: &Member,
    occurrences: &[Occurrence],
) -> Vec<Finding> {
    let mut findings: Vec<Finding> = occurrences
        .iter()
        .map(|occurrence| Finding::UnsafeInConsumer {
            crate_name: member.name.clone(),
            occurrence: occurrence.clone(),
        })
        .collect();
    findings.extend(missing_opt_outs(root, member));
    findings
}

/// Every `unsafe` in one file.
///
/// A line scan, not a parse: it is the shape of the thing the policy
/// governs, and the workspace's `unsafe_code` lint is what actually stops
/// an `unsafe` compiling. What this adds is reading the source rather than
/// the lint level, so an `unsafe` a hand-written `allow` would let through
/// is still found.
fn occurrences(root: &Path, source: &Path) -> Vec<Occurrence> {
    let text = std::fs::read_to_string(source)
        .unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
    let lines: Vec<&str> = text.lines().collect();
    // Carried rather than chained: a string or comment opened on one line
    // decides how the next one reads, which a per-line filter cannot see.
    let mut carry = lexical::Carry::Code;
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let (code, next) = lexical::blank_quoted(line, carry);
        carry = next;
        if opens_unsafe(&code) {
            found.push(Occurrence {
                file: relative(root, source),
                line: index + 1,
                text: (*line).trim().to_owned(),
                kind: kind(&code),
                has_safety_comment: has_safety_comment(&lines[..index]),
            });
        }
    }
    found
}

/// Whether a line opens an `unsafe` block, function, implementation or
/// `extern` declaration.
///
/// The word has to start a token, so `unsafe_code` and `unsafePerm` do not
/// match, and it has to be followed by what makes it an operation rather
/// than prose, so a comment or a string mentioning it does not either.
fn opens_unsafe(line: &str) -> bool {
    let trimmed = line.trim_start();
    // A comment or a block-comment continuation describes an `unsafe`
    // rather than being one. The continuation is a `*` that ends the line
    // or is followed by a space or the `/` that closes the comment, which
    // is what tells it from a dereference such as `*slot = unsafe { x }`.
    let continues_block_comment = trimmed
        .strip_prefix('*')
        .is_some_and(|rest| {
            rest.is_empty() || rest.starts_with('/') || rest.starts_with(char::is_whitespace)
        });
    if trimmed.starts_with("//") || continues_block_comment {
        return false;
    }
    unsafe_tokens(line)
        .any(|rest| {
            rest.starts_with('{')
                || rest.starts_with("fn ")
                || rest.starts_with("impl ")
                || rest.starts_with("trait ")
                || rest.starts_with("extern")
        })
}

/// What the `unsafe` on a line is.
fn kind(line: &str) -> Kind {
    let mut kind = Kind::Operation;
    for rest in unsafe_tokens(line) {
        let Some(rest) = rest.strip_prefix("extern") else { continue };
        // `unsafe extern {` or `unsafe extern "abi" {` declares an ABI;
        // `unsafe extern "abi" fn` states one signature.
        let declares = rest
            .trim_start()
            .trim_start_matches(|character: char| character != '{' && character != 'f')
            .starts_with('{');
        kind = if declares { Kind::ForeignAbiBlock } else { Kind::ForeignSignature };
    }
    kind
}

/// What follows each `unsafe` token on a line, with the leading space
/// taken off.
///
/// A token inside a string literal is left out, because the scanner's own
/// fixtures are lines of Rust quoted as data and a diagnostic can quote
/// one too.
fn unsafe_tokens(line: &str) -> impl Iterator<Item = &str> {
    line.match_indices("unsafe")
        .filter(move |(index, _)| !preceded_by_word(line, *index) && !quoted(line, *index))
        .map(move |(index, token)| line[index + token.len()..].trim_start())
}

/// Whether the character before `index` continues an identifier, which
/// makes the match part of a longer name such as `unsafe_code`.
fn preceded_by_word(line: &str, index: usize) -> bool {
    line[..index]
        .chars()
        .next_back()
        .is_some_and(|character| character.is_alphanumeric() || character == '_')
}

/// Whether `index` falls inside a string literal, which an odd number of
/// unescaped quotes before it means.
///
/// A line-local count, not a lexer: a raw string or a multi-line literal
/// can fool it. What that costs is a wrong answer on a line that mentions
/// `unsafe` inside such a literal, and the attribute check is what
/// actually keeps `unsafe` out of a crate.
fn quoted(line: &str, index: usize) -> bool {
    let before = &line[..index];
    if opens_raw_string(before) {
        return true;
    }
    let mut quotes = 0;
    let mut escaped = false;
    for character in before.chars() {
        match character {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => quotes += 1,
            _ => {}
        }
    }
    quotes % 2 == 1
}

/// Whether a raw string opens in `before`.
///
/// A raw string takes no escapes, so the quote count above cannot be read
/// through one. Everything after the opener on the line is treated as
/// inside it, which is why this is checked first.
fn opens_raw_string(before: &str) -> bool {
    before
        .match_indices('r')
        .filter(|(index, _)| !preceded_by_word(before, *index))
        .any(|(index, _)| before[index + 1..].trim_start_matches('#').starts_with('"'))
}

/// Whether the comment block above an `unsafe` carries a safety line: one
/// whose first word is the `SAFETY` marker.
///
/// The whole block, not just the line above, because a safety argument
/// that names every precondition rarely fits on one. Three things may sit
/// between the comment and the `unsafe`: a blank line; an attribute, since
/// an `#[expect(...)]` or a `#[cfg(...)]` often does; and a line ending in
/// `=`, which is where the formatter wraps a long `let` whose value is the
/// `unsafe`.
fn has_safety_comment(above: &[&str]) -> bool {
    above
        .iter()
        .rev()
        .map(|line| line.trim())
        .skip_while(|line| line.is_empty() || line.starts_with('#') || line.ends_with('='))
        .take_while(|line| line.starts_with("//"))
        .any(|line| line.starts_with("// SAFETY:"))
}

/// Whether a crate root opts out of the `unsafe_code` lint.
fn opts_out_of_unsafe_code(root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(root) else {
        return false;
    };
    text.lines().any(opts_out)
}

/// Whether a line is an inner attribute opting out of the `unsafe_code`
/// lint.
///
/// The attribute has to open the line, which leaves out a doc comment
/// describing one and a fixture quoting one as data. It has to be the
/// inner form too: an outer `#[allow(unsafe_code)]` covers one item rather
/// than the crate, and an `unsafe` it lets through is caught by the scan
/// that reads the source instead of the lint level.
fn opts_out(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("#![allow(unsafe_code") || line.starts_with("#![expect(unsafe_code")
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_path_buf()
}

mod lexical;

#[cfg(test)]
mod tests;
