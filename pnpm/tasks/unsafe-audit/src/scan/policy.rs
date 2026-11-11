//! The policy's rules, applied to what the scan found.

use super::{
    Finding, Kind, Occurrence,
    source::{occurrences, opts_out_of_unsafe_code, relative},
};
use crate::{Exemption, Member, Tier, exemption::exemption_for};
use std::path::Path;

/// The check that is the workspace's rather than any member's: a member
/// inheriting the lint table means nothing unless the table lints
/// `unsafe_code`.
pub(super) fn workspace_findings(root: &Path) -> Vec<Finding> {
    let level = crate::crates::workspace_unsafe_code_level(root);
    let linted = level
        .as_deref()
        .is_some_and(|level| matches!(level, "warn" | "deny" | "forbid"));
    if linted {
        return Vec::new();
    }
    vec![match level {
        Some(level) => Finding::UnsafeCodeLintedTooWeakly { level },
        None => Finding::UnsafeCodeNotLinted,
    }]
}

pub(super) fn audit_member(root: &Path, member: &Member) -> Vec<Finding> {
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
