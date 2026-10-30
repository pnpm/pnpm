use super::{Finding, Kind, Occurrence};
use text_block_macros::text_block;

fn occurrence() -> Occurrence {
    Occurrence {
        file: std::path::PathBuf::from("pnpm/crates/demo/src/lib.rs"),
        line: 42,
        text: "unsafe { read() }".to_owned(),
        kind: Kind::Operation,
        has_safety_comment: false,
    }
}

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from("pnpm/crates/demo/src/lib.rs")
}

#[test]
fn an_occurrence_reads_as_a_place_and_the_line_there() {
    assert_eq!(occurrence().to_string(), "pnpm/crates/demo/src/lib.rs:42: unsafe { read() }");
}

/// The audit prints these and nothing else, so each is the whole of what a
/// contributor who trips the gate gets told.
#[test]
fn unsafe_in_a_consumer_says_where_the_call_belongs() {
    assert_eq!(
        Finding::UnsafeInConsumer { crate_name: "demo".to_owned(), occurrence: occurrence() }
            .to_string(),
        text_block! {
            "demo is not exempt from the unsafe policy, but holds `unsafe` at pnpm/crates/demo/src/lib.rs:42: unsafe { read() }"
            "    Move the call into the abstraction crate that owns the surface, or add a new one for a surface that has no owner yet."
        },
    );
}

#[test]
fn a_weak_workspace_lint_names_the_level_it_found() {
    assert_eq!(
        Finding::UnsafeCodeLintedTooWeakly { level: "allow".to_owned() }.to_string(),
        text_block! {
            "the workspace manifest lints `unsafe_code` at `allow`, which does not hold a crate to the policy."
            r#"    `[workspace.lints.rust]` needs at least `unsafe_code = "warn"`; CI's `--deny warnings` makes it an error."#
        },
    );
}

#[test]
fn a_missing_workspace_lint_says_nothing_stops_an_unsafe() {
    assert_eq!(
        Finding::UnsafeCodeNotLinted.to_string(),
        text_block! {
            "the workspace manifest does not lint `unsafe_code`, so nothing stops an `unsafe` compiling."
            r#"    `[workspace.lints.rust]` needs at least `unsafe_code = "warn"`; CI's `--deny warnings` makes it an error."#
        },
    );
}

#[test]
fn a_member_outside_the_lint_table_is_told_which_table() {
    assert_eq!(
        Finding::NotInheritingWorkspaceLints { crate_name: "demo".to_owned() }.to_string(),
        text_block! {
            "demo does not take the workspace lint table, so the `unsafe_code` lint does not reach it."
            "    Its manifest needs a `[lints]` table with `workspace = true`."
        },
    );
}

#[test]
fn a_missing_opt_out_names_the_crate_root() {
    assert_eq!(
        Finding::MissingOptOut { crate_name: "demo".to_owned(), root: root() }.to_string(),
        text_block! {
            "demo is exempt from the unsafe policy, so pnpm/crates/demo/src/lib.rs needs an `allow(unsafe_code)` inner attribute."
            "    Without it the `unsafe` the crate exists to hold is a warning, and CI's `--deny warnings` an error."
        },
    );
}

#[test]
fn an_unsanctioned_opt_out_names_the_crate_root_too() {
    assert_eq!(
        Finding::UnsanctionedOptOut { crate_name: "demo".to_owned(), root: root() }.to_string(),
        text_block! {
            "demo is not exempt from the unsafe policy, but pnpm/crates/demo/src/lib.rs opts out of the `unsafe_code` lint."
            "    Drop the attribute, or add the crate to `exempt()` if the policy really does let it hold `unsafe`."
        },
    );
}

#[test]
fn a_stale_exemption_says_to_drop_both_halves() {
    assert_eq!(
        Finding::StaleExemption { crate_name: "demo".to_owned() }.to_string(),
        text_block! {
            "demo is exempt from the unsafe policy but holds no `unsafe`."
            "    Drop its entry from `exempt()`, and the opt-out from its crate root."
        },
    );
}

/// These two carry no remedy, because the statement already says what to do,
/// so each is one line.
#[test]
fn a_foreign_abi_outside_a_sys_crate_is_one_line() {
    assert_eq!(
        Finding::ForeignAbiOutsideSysCrate {
            crate_name: "demo".to_owned(),
            occurrence: occurrence(),
        }
        .to_string(),
        "demo is not a `-sys` crate, but declares a foreign ABI at pnpm/crates/demo/src/lib.rs:42: unsafe { read() }",
    );
}

#[test]
fn a_missing_safety_comment_is_one_line() {
    assert_eq!(
        Finding::MissingSafetyComment { crate_name: "demo".to_owned(), occurrence: occurrence() }
            .to_string(),
        "demo has an `unsafe` with no `// SAFETY:` comment above it, at pnpm/crates/demo/src/lib.rs:42: unsafe { read() }",
    );
}
