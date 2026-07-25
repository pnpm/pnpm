use super::passthrough_from;
use std::ffi::OsString;

/// `argv` split at the boundary: what pnpm parses, and what it forwards.
fn split(argv: &[&str]) -> (Vec<String>, Vec<String>) {
    let owned: Vec<OsString> =
        std::iter::once("pnpm").chain(argv.iter().copied()).map(OsString::from).collect();
    let boundary = passthrough_from(&owned).unwrap_or(owned.len());
    let to_string = |slot: &OsString| slot.to_string_lossy().into_owned();
    // Skip the `pnpm` slot in the parsed half; it is never interesting.
    (
        owned[1..boundary.max(1)].iter().map(to_string).collect(),
        owned[boundary.min(owned.len())..].iter().map(to_string).collect(),
    )
}

#[test]
fn a_command_taking_a_foreign_command_line_forwards_everything_after_its_first_positional() {
    for (argv, forwarded) in [
        (["run", "show", "--config.foo=bar"].as_slice(), ["--config.foo=bar"].as_slice()),
        (&["run", "show", "--silent"], &["--silent"]),
        (&["run", "show", "-s", "--if-present"], &["-s", "--if-present"]),
        (&["exec", "node", "show.js", "--config.foo=bar"], &["show.js", "--config.foo=bar"]),
        (&["dlx", "cowsay", "--silent"], &["--silent"]),
        // The recursive prefix forms defer the classification one positional.
        (&["recursive", "run", "show", "--silent"], &["--silent"]),
        (&["m", "run", "show", "--silent"], &["--silent"]),
    ] {
        let (_, actual) = split(argv);
        assert_eq!(actual, forwarded, "{argv:?}");
    }
}

#[test]
fn options_before_the_script_name_stay_pnpms() {
    for argv in [
        ["run", "--silent", "show"].as_slice(),
        &["run", "--config.foo=bar", "show"],
        &["-r", "run", "show"],
        &["--dir", "/tmp", "run", "show"],
        &["--filter", "pkg", "exec", "node"],
        &["-C", "/tmp", "run", "show"],
    ] {
        let (_, forwarded) = split(argv);
        assert!(forwarded.is_empty(), "{argv:?} forwards {forwarded:?}");
    }
}

#[test]
fn a_command_parsing_its_own_arguments_owns_all_of_argv() {
    for argv in [
        ["install", "--config.foo=bar"].as_slice(),
        &["add", "foo", "--config.foo=bar"],
        &["list", "--silent"],
        // A package that happens to share an escaping command's name is
        // still just an argument to `add`.
        &["add", "run", "--config.foo=bar"],
    ] {
        let (_, forwarded) = split(argv);
        assert!(forwarded.is_empty(), "{argv:?} forwards {forwarded:?}");
    }
}

#[test]
fn the_fallback_script_forwards_everything_after_its_name() {
    let (_, forwarded) = split(&["some-script", "--config.foo=bar"]);
    assert_eq!(forwarded, ["--config.foo=bar"]);
}

#[test]
fn an_explicit_separator_ends_parsing_wherever_it_appears() {
    for (argv, forwarded) in [
        (["install", "--", "--config.foo=bar"].as_slice(), ["--config.foo=bar"].as_slice()),
        // The script name stays on pnpm's side; the separator and
        // everything after it are the script's.
        (&["run", "show", "--", "--config.foo=bar"], &["--", "--config.foo=bar"]),
        (&["--", "--config.foo=bar"], &["--config.foo=bar"]),
    ] {
        let (_, actual) = split(argv);
        assert_eq!(actual, forwarded, "{argv:?}");
    }
}

/// A command given no positional at all has nothing to forward: bare
/// `pnpm run` lists scripts.
#[test]
fn a_foreign_command_line_without_a_positional_forwards_nothing() {
    for argv in [["run"].as_slice(), &["run", "--silent"], &["exec"]] {
        let (_, forwarded) = split(argv);
        assert!(forwarded.is_empty(), "{argv:?} forwards {forwarded:?}");
    }
}

/// Arity has to come from clap: a value-taking option whose value looks
/// like a positional would otherwise land the boundary on that value and
/// forward pnpm's own later flags to the child.
#[test]
fn a_value_taking_option_does_not_move_the_boundary_onto_its_value() {
    for (argv, forwarded) in [
        // `--package` is `dlx`'s own option, not a global one.
        (
            ["dlx", "--package", "cowsay", "--silent", "cowsay", "--moo", "hi"].as_slice(),
            ["--moo", "hi"].as_slice(),
        ),
        (&["dlx", "--package", "cowsay", "cowsay", "--silent"], &["--silent"]),
        (&["dlx", "--allow-build", "esbuild", "cowsay", "--moo"], &["--moo"]),
        // Value-taking globals, including ones no hand-written list had.
        (&["--workspace-concurrency", "2", "install", "--config.registry=x"], &[]),
        (&["--test-pattern", "**/*.spec.ts", "install", "--config.foo=bar"], &[]),
        (&["--resume-from", "pkg", "-r", "run", "build", "--silent"], &["--silent"]),
    ] {
        let (_, actual) = split(argv);
        assert_eq!(actual, forwarded, "{argv:?}");
    }
}

/// `with` splits on its version, because only some of its invocations hand
/// their arguments to another process.
///
/// `with current` is spliced into pnpm's own argv by `with_current::rewrite`
/// *after* the pre-clap passes, so an override in it is still pnpm's to
/// claim — leaving it would let clap mistake it for the script name. Any
/// other version execs a child pnpm, so stripping an override would lose it
/// for that child.
#[test]
fn with_forwards_only_for_a_version_that_execs_a_child() {
    // `with current` resumes the scan on the nested command line, so the
    // nested boundary is what applies: ahead of the inner script name the
    // tokens are pnpm's, past it they are the script's.
    let (_, before) = split(&["with", "current", "run", "--config.foo=bar", "build"]);
    assert!(before.is_empty(), "an override ahead of the script name is pnpm's: {before:?}");

    for (argv, forwarded) in [
        (
            ["with", "current", "run", "show", "--config.foo=bar"].as_slice(),
            ["--config.foo=bar"].as_slice(),
        ),
        (&["with", "current", "run", "show", "--silent"], &["--silent"]),
        (&["with", "current", "recursive", "run", "show", "--silent"], &["--silent"]),
        // A nested command that parses its own arguments still owns them.
        (&["with", "current", "install", "--config.foo=bar"], &[]),
    ] {
        let (_, actual) = split(argv);
        assert_eq!(actual, forwarded, "{argv:?}");
    }

    // The whole child command line, command included — forwarding only the
    // options would leave the child pnpm with nothing to run.
    for (argv, forwarded) in [
        (
            ["with", "10", "install", "--config.foo=bar"].as_slice(),
            ["install", "--config.foo=bar"].as_slice(),
        ),
        (&["with", "pnpm@9", "install", "--silent"], &["install", "--silent"]),
    ] {
        let (_, actual) = split(argv);
        assert_eq!(actual, forwarded, "{argv:?}");
    }
}
