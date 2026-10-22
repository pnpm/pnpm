//! Reports where the workspace departs from the `unsafe` policy.
//!
//! The same audit `tests/policy.rs` runs, as a command, so that a
//! contributor fixing a batch of findings can see them all at once instead
//! of through a failing assertion.
//!
//! `--roots` prints only the crate roots that are missing
//! `#![forbid(unsafe_code)]`, one per line, which is the form a script can
//! act on.

#![forbid(unsafe_code)]
#![expect(clippy::print_stdout, reason = "developer tool, not part of the pnpm binary")]

use pnpm_unsafe_audit::{Finding, audit, workspace_root};

fn main() -> std::process::ExitCode {
    let roots_only = std::env::args().any(|argument| argument == "--roots");
    let findings = audit(&workspace_root());
    if roots_only {
        for finding in &findings {
            if let Finding::MissingForbid { root, .. } = finding {
                println!("{}", root.display());
            }
        }
    } else {
        for finding in &findings {
            println!("{finding}\n");
        }
    }
    if findings.is_empty() {
        println!("The workspace holds to the unsafe policy.");
        return std::process::ExitCode::SUCCESS;
    }
    std::process::ExitCode::FAILURE
}
