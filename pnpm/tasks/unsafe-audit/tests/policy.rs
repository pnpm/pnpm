//! The policy, as a test.

#![forbid(unsafe_code)]

use pnpm_unsafe_audit::{audit, workspace_root};

/// Every workspace member either is exempt from the `unsafe` policy and
/// holds `unsafe` for the reason its exemption gives, or forbids `unsafe`
/// outright.
///
/// The failure names each departure and what to do about it. Read
/// `UNSAFE_POLICY.md` before adding an exemption to make one pass.
#[test]
fn the_workspace_holds_to_the_unsafe_policy() {
    let findings = audit(&workspace_root());
    assert!(
        findings.is_empty(),
        "the workspace departs from the unsafe policy:\n\n{}\n",
        findings
            .iter()
            .map(|finding| format!("  - {finding}"))
            .collect::<Vec<_>>()
            .join("\n\n"),
    );
}
