//! Reading the policy off the source.

use crate::{Member, Tier, exemption::exemption_for};
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
    /// A crate the policy does not exempt is missing the attribute that
    /// would stop one being added.
    MissingForbid { crate_name: String, root: PathBuf },
    /// A crate whose macros expand to `unsafe` is missing the attribute
    /// that would stop a hand-written one being added.
    MissingDeny { crate_name: String, root: PathBuf },
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
        match self {
            Self::UnsafeInConsumer { crate_name, occurrence } => write!(
                formatter,
                "{crate_name} is not exempt from the unsafe policy, but holds `unsafe` at \
                 {occurrence}\n    Move the call into the abstraction crate that owns the \
                 surface, or add a new one for a surface that has no owner yet.",
            ),
            Self::MissingForbid { crate_name, root } => write!(
                formatter,
                "{crate_name} is not exempt from the unsafe policy, so {} needs \
                 `#![forbid(unsafe_code)]`",
                root.display(),
            ),
            Self::MissingDeny { crate_name, root } => write!(
                formatter,
                "{crate_name} writes no `unsafe` of its own, so {} needs \
                 `#![deny(unsafe_code)]`.\n    `forbid` would reject its macro expansion's \
                 own `#[allow(unsafe_code)]`; `deny` lets that through and stops everything \
                 else.",
                root.display(),
            ),
            Self::StaleExemption { crate_name } => write!(
                formatter,
                "{crate_name} is exempt from the unsafe policy but holds no `unsafe`.\n    \
                 Drop its entry from `exempt()` and give it `#![forbid(unsafe_code)]`.",
            ),
            Self::ForeignAbiOutsideSysCrate { crate_name, occurrence } => write!(
                formatter,
                "{crate_name} is not a `-sys` crate, but declares a foreign ABI at {occurrence}",
            ),
            Self::MissingSafetyComment { crate_name, occurrence } => write!(
                formatter,
                "{crate_name} has an `unsafe` with no `// SAFETY:` comment above it, at \
                 {occurrence}",
            ),
        }
    }
}

/// Every way the workspace rooted at `root` departs from the policy.
///
/// An empty answer is the policy holding.
#[must_use]
pub fn audit(root: &Path) -> Vec<Finding> {
    crate::members(root)
        .iter()
        .flat_map(|member| audit_member(root, member))
        .collect()
}

fn audit_member(root: &Path, member: &Member) -> Vec<Finding> {
    let occurrences: Vec<Occurrence> = member.sources
        .iter()
        .flat_map(|source| occurrences(root, source))
        .collect();
    let Some(exemption) = exemption_for(&member.name) else {
        return consumer_findings(root, member, occurrences);
    };
    if exemption.tier == Tier::MacroGenerated {
        return macro_generated_findings(root, member, &occurrences);
    }
    if occurrences.is_empty() {
        return vec![Finding::StaleExemption { crate_name: member.name.clone() }];
    }
    occurrences
        .into_iter()
        .filter_map(|occurrence| match occurrence.kind {
            Kind::ForeignAbiBlock => (exemption.tier != Tier::ForeignAbi).then(|| {
                Finding::ForeignAbiOutsideSysCrate { crate_name: member.name.clone(), occurrence }
            }),
            Kind::ForeignSignature => None,
            Kind::Operation => (!occurrence.has_safety_comment).then(|| {
                Finding::MissingSafetyComment { crate_name: member.name.clone(), occurrence }
            }),
        })
        .collect()
}

/// A crate the policy does not exempt may hold no `unsafe` at all, and
/// needs the attribute that keeps it that way in each of its crate roots.
fn consumer_findings(root: &Path, member: &Member, occurrences: Vec<Occurrence>) -> Vec<Finding> {
    let mut findings: Vec<Finding> = occurrences
        .into_iter()
        .map(|occurrence| Finding::UnsafeInConsumer { crate_name: member.name.clone(), occurrence })
        .collect();
    findings.extend(
        member.roots
            .iter()
            .filter(|crate_root| !forbids_unsafe(crate_root))
            .map(|missing| Finding::MissingForbid {
                crate_name: member.name.clone(),
                root: relative(root, missing),
            }),
    );
    findings
}

/// A crate whose macros expand to `unsafe` writes none of its own, so the
/// scan's job there is the opposite: every occurrence is hand-written and
/// so a finding, and the attribute it needs is `deny` rather than `forbid`.
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
    findings.extend(
        member.roots
            .iter()
            .filter(|crate_root| !denies_unsafe(crate_root))
            .map(|missing| Finding::MissingDeny {
                crate_name: member.name.clone(),
                root: relative(root, missing),
            }),
    );
    findings
}

/// Every `unsafe` in one file.
///
/// A line scan, not a parse: it is the shape of the thing the policy
/// governs, and `#![forbid(unsafe_code)]` is what actually stops an
/// `unsafe` compiling. What this adds is catching the crate that has
/// neither.
fn occurrences(root: &Path, source: &Path) -> Vec<Occurrence> {
    let Ok(text) = std::fs::read_to_string(source) else { return Vec::new() };
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| opens_unsafe(line))
        .map(|(index, line)| Occurrence {
            file: relative(root, source),
            line: index + 1,
            text: (*line).trim().to_owned(),
            kind: kind(line),
            has_safety_comment: has_safety_comment(&lines[..index]),
        })
        .collect()
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
    // rather than being one.
    if trimmed.starts_with("//") || trimmed.starts_with('*') {
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

/// Whether a crate root forbids `unsafe`.
fn forbids_unsafe(root: &Path) -> bool {
    has_attribute(root, "#![forbid(unsafe_code)]")
}

/// Whether a crate root denies `unsafe`.
fn denies_unsafe(root: &Path) -> bool {
    has_attribute(root, "#![deny(unsafe_code)]")
}

fn has_attribute(root: &Path, attribute: &str) -> bool {
    std::fs::read_to_string(root).is_ok_and(|text| text.contains(attribute))
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_path_buf()
}

#[cfg(test)]
mod tests;
