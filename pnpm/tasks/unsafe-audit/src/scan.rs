//! Reading the policy off the source.

pub use finding::{Finding, Kind, Occurrence};

use policy::{audit_member, workspace_findings};
use std::path::Path;

/// Every way the workspace rooted at `root` departs from the policy.
///
/// An empty answer is the policy holding.
#[must_use]
pub fn audit(root: &Path) -> Vec<Finding> {
    let mut findings = workspace_findings(root);
    findings.extend(crate::members(root).iter().flat_map(|member| audit_member(root, member)));
    findings
}

mod finding;
mod lexical;
mod policy;
mod source;
