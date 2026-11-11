//! Reports where the workspace departs from the `unsafe` policy.
//!
//! The same audit `tests/policy.rs` runs, as a command, so that a
//! contributor fixing a batch of findings can see them all at once instead
//! of through a failing assertion.

#![expect(clippy::print_stdout, reason = "developer tool, not part of the pnpm binary")]

use pnpm_unsafe_audit::{audit, workspace_root};

fn main() -> std::process::ExitCode {
    let findings = audit(&workspace_root());
    for finding in &findings {
        println!("{finding}\n");
    }
    if findings.is_empty() {
        println!("The workspace holds to the unsafe policy.");
        return std::process::ExitCode::SUCCESS;
    }
    std::process::ExitCode::FAILURE
}
