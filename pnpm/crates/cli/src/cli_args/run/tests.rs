use super::{RunError, ScriptSelector, render_project_commands, throw_or_filter_hidden_scripts};
use clap::Parser;
use serde_json::json;

#[test]
fn specified_scripts_exact_match() {
    let manifest = json!({ "scripts": { "build": "tsc", "test": "jest" } });
    assert_eq!(ScriptSelector::new("build").unwrap().select(&manifest), vec!["build".to_string()]);
    assert_eq!(ScriptSelector::new("test").unwrap().select(&manifest), vec!["test".to_string()]);
}

#[test]
fn specified_scripts_start_fallback() {
    let manifest = json!({ "scripts": { "build": "tsc" } });
    assert_eq!(
        ScriptSelector::new("start").unwrap().select_with_start(&manifest),
        vec!["start".to_string()],
    );
    assert!(
        ScriptSelector::new("start").unwrap().select(&manifest).is_empty(),
        "the fallback belongs to `run`, not to the recursive selector",
    );
}

#[test]
fn specified_scripts_missing_is_empty() {
    let manifest = json!({ "scripts": { "build": "tsc" } });
    assert!(ScriptSelector::new("nonexistent").unwrap().select(&manifest).is_empty());
}

#[test]
fn specified_scripts_selects_every_regexp_match() {
    let manifest = json!({
        "scripts": {
            "build:backend": "tsc",
            "build:frontend": "vite build",
            "build": "echo all",
            "typecheck": "tsc --noEmit",
        },
    });
    assert_eq!(
        ScriptSelector::new("/^build:(backend|frontend)$/").unwrap().select(&manifest),
        vec!["build:backend".to_string(), "build:frontend".to_string()],
    );
    // The pattern is not implicitly anchored to the whole script name —
    // it is searched for — so `build` matches this one too, and the
    // matches keep the manifest's declaration order.
    assert_eq!(
        ScriptSelector::new("/^build/").unwrap().select(&manifest),
        vec!["build:backend".to_string(), "build:frontend".to_string(), "build".to_string()],
    );
}

/// An exact hit wins over the regexp reading, so a script literally named
/// like a regexp literal stays runnable.
#[test]
fn specified_scripts_prefers_an_exact_match_over_the_pattern() {
    let manifest = json!({ "scripts": { "/^a/": "echo literal", "ab": "echo matched" } });
    assert_eq!(ScriptSelector::new("/^a/").unwrap().select(&manifest), vec!["/^a/".to_string()]);
}

#[test]
fn specified_scripts_rejects_regexp_flags() {
    // Rejected while building the selector, before any manifest is read,
    // so a recursive run reports it once rather than per project.
    let err = ScriptSelector::new("/^BUILD/i").expect_err("flags are rejected");
    assert!(matches!(err, RunError::UnsupportedScriptCommandFormat), "got {err:?}");
}

/// Anything that isn't a well-formed regexp literal — a bare `/`-bearing
/// name, an empty pattern, or a pattern the engine can't compile — reads
/// as a plain script name and finds nothing.
#[test]
fn specified_scripts_treats_non_literals_as_names() {
    let manifest = json!({ "scripts": { "build": "tsc" } });
    for name in ["/a/b/", "//", "/build", "build/", "/[/"] {
        assert!(
            ScriptSelector::new(name).unwrap().select(&manifest).is_empty(),
            "{name} is not a regexp selector",
        );
    }
}

#[test]
fn hidden_filter_passes_visible_scripts() {
    let scripts = vec!["build".to_string()];
    assert_eq!(throw_or_filter_hidden_scripts(scripts.clone(), "build").unwrap(), scripts);
}

#[test]
fn hidden_filter_rejects_exact_hidden_request() {
    let scripts = vec![".secret".to_string()];
    let err = throw_or_filter_hidden_scripts(scripts, ".secret").unwrap_err();
    assert!(matches!(err, RunError::HiddenScript { .. }), "got {err:?}");
}

#[test]
fn hidden_filter_all_hidden_yields_all_hidden_error() {
    let scripts = vec![".a".to_string(), ".b".to_string()];
    let err = throw_or_filter_hidden_scripts(scripts, "any").unwrap_err();
    assert!(matches!(err, RunError::AllHidden { .. }), "got {err:?}");
}

#[test]
fn print_commands_groups_lifecycle_and_other() {
    let manifest = json!({
        "scripts": { "test": "jest", "build": "tsc", ".hidden": "secret" },
    });
    let output = render_project_commands(&manifest, None);
    assert!(output.contains("Lifecycle scripts:"), "lifecycle header:\n{output}");
    assert!(output.contains("  test\n    jest"), "test under lifecycle:\n{output}");
    assert!(output.contains(r#"Commands available via "pnpm run":"#), "other header:\n{output}");
    assert!(output.contains("  build\n    tsc"), "build under other:\n{output}");
    assert!(!output.contains("hidden"), "hidden scripts are omitted:\n{output}");
}

#[test]
fn print_commands_empty_when_no_scripts() {
    let manifest = json!({ "name": "x" });
    let output = render_project_commands(&manifest, None);
    assert_eq!(output, "There are no scripts specified.");
}

/// Everything after the script name reaches the script verbatim, so a
/// `--` separator survives and a pnpm-flag-shaped argument is not claimed
/// by pnpm (pnpm/pnpm#13295). pnpm gets this by listing `run` in
/// `SPECIALLY_ESCAPED_CMDS`; here it falls out of the single
/// `trailing_var_arg` positional.
#[test]
fn run_forwards_every_token_after_the_script_name() {
    for (argv, script_args) in [
        (["run", "show", "--", "--other=1"].as_slice(), ["--", "--other=1"].as_slice()),
        (&["run", "show", "--other=1"], &["--other=1"]),
        (&["run", "show", "--", "-s"], &["--", "-s"]),
        (&["run", "show", "--", "a", "b"], &["--", "a", "b"]),
        (&["run", "show", "--", "--"], &["--", "--"]),
        // `-s` / `--if-present` are pnpm's own `run` flags, but after the
        // script name they belong to the script.
        (&["run", "show", "-s"], &["-s"]),
        (&["run", "show", "--if-present"], &["--if-present"]),
        (&["run", "show"], &[]),
    ] {
        let args = run_args(argv);

        assert_eq!(args.script_name(), Some("show"), "{argv:?}");
        assert_eq!(args.script_args(), script_args, "{argv:?}");
        assert!(!args.sequential, "{argv:?}: -s after the script name is not --sequential");
        assert!(!args.if_present, "{argv:?}: --if-present after the script name is the script's");
    }
}

/// The same flags ahead of the script name are still pnpm's.
#[test]
fn run_flags_before_the_script_name_are_pnpms() {
    let sequential = run_args(&["run", "-s", "show"]);
    assert!(sequential.sequential);
    assert_eq!(sequential.script_name(), Some("show"));
    assert!(sequential.script_args().is_empty());

    let if_present = run_args(&["run", "--if-present", "show", "x"]);
    assert!(if_present.if_present);
    assert_eq!(if_present.script_name(), Some("show"));
    assert_eq!(if_present.script_args(), ["x"]);
}

/// `run` with no positional lists the available scripts rather than
/// running one.
#[test]
fn run_without_a_script_name_has_none() {
    let args = run_args(&["run"]);
    assert_eq!(args.script_name(), None);
    assert!(args.script_args().is_empty());
}

fn run_args(argv: &[&str]) -> super::RunArgs {
    let parsed = crate::cli_args::CliArgs::try_parse_from(
        std::iter::once("pnpm").chain(argv.iter().copied()),
    )
    .unwrap_or_else(|error| panic!("{argv:?} should parse: {error}"));
    match parsed.command {
        crate::cli_args::cli_command::CliCommand::Run(args) => args,
        other => panic!("{argv:?} should parse as run, got {other:?}"),
    }
}
