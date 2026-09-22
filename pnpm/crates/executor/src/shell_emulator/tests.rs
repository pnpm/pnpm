use super::{
    EmulatedOutput,
    ShellEmulatorError,
    execute_emulated,
};
use pnpm_reporter::LifecycleStdio;
use std::{
    collections::HashMap,
    path::Path,
    sync::Mutex,
};
use tempfile::tempdir;

/// Run `script` in `cwd`, returning its exit code and the lines it wrote,
/// each tagged with the stream it came from.
fn run(
    script: &str,
    cwd: &Path,
    env: &HashMap<String, String>,
) -> (i32, Vec<(LifecycleStdio, String)>) {
    let lines = Mutex::new(Vec::new());
    let sink = |stdio, line| {
        lines
            .lock()
            .expect("the sink is never poisoned")
            .push((stdio, line));
    };
    let code = execute_emulated(script, cwd, env, EmulatedOutput::Lines(&sink), None)
        .expect("run the script under the emulator");
    (code, lines.into_inner().expect("the sink is never poisoned"))
}

/// The lines `stdio` carried, in the order the sink saw them.
fn lines_from(lines: &[(LifecycleStdio, String)], stdio: LifecycleStdio) -> Vec<&str> {
    lines
        .iter()
        .filter(|(line_stdio, _)| *line_stdio == stdio)
        .map(|(_, line)| line.as_str())
        .collect()
}

#[test]
fn captures_stdout_line_by_line() {
    let dir = tempdir().expect("create a temp dir");
    let (code, lines) = run("echo first && echo second", dir.path(), &HashMap::new());
    dbg!(&lines);
    assert_eq!(code, 0);
    assert_eq!(
        lines,
        vec![
            (LifecycleStdio::Stdout, "first".to_string()),
            (LifecycleStdio::Stdout, "second".to_string()),
        ],
    );
}

/// Each stream keeps its own order, but the two are pumped
/// independently, so which of them reaches the sink first is a race and
/// is deliberately not asserted.
#[test]
fn separates_stderr_from_stdout() {
    let dir = tempdir().expect("create a temp dir");
    let (code, lines) = run("echo out && echo err 1>&2", dir.path(), &HashMap::new());
    dbg!(&lines);
    assert_eq!(code, 0);
    assert_eq!(lines_from(&lines, LifecycleStdio::Stdout), vec!["out"]);
    assert_eq!(lines_from(&lines, LifecycleStdio::Stderr), vec!["err"]);
}

/// A script's last line often arrives without a trailing newline; it must
/// still reach the sink rather than being dropped at EOF.
#[test]
fn emits_a_final_line_without_a_newline() {
    let dir = tempdir().expect("create a temp dir");
    std::fs::write(dir.path().join("unterminated.txt"), "trailing")
        .expect("write a file with no trailing newline");
    let (code, lines) = run("cat unterminated.txt", dir.path(), &HashMap::new());
    dbg!(&lines);
    assert_eq!(code, 0);
    assert_eq!(lines, vec![(LifecycleStdio::Stdout, "trailing".to_string())]);
}

#[test]
fn reports_the_scripts_exit_code() {
    let dir = tempdir().expect("create a temp dir");
    let (code, _) = run("exit 3", dir.path(), &HashMap::new());
    assert_eq!(code, 3);
}

/// The emulator resolves variables against the environment the caller
/// built for the script, not against pacquet's own environment.
#[test]
fn expands_variables_from_the_supplied_env() {
    let dir = tempdir().expect("create a temp dir");
    let env = HashMap::from([("npm_package_name".to_string(), "my-pkg".to_string())]);
    let (code, lines) = run("echo $npm_package_name", dir.path(), &env);
    dbg!(&lines);
    assert_eq!(code, 0);
    assert_eq!(lines, vec![(LifecycleStdio::Stdout, "my-pkg".to_string())]);
}

#[test]
fn expands_braced_parameters() {
    let dir = tempdir().expect("create a temp dir");
    let env = HashMap::from([
        ("MY_VAR".to_string(), "hello".to_string()),
        ("EMPTY".to_string(), String::new()),
    ]);

    for (script, expected) in [
        ("echo ${MY_VAR}", "hello"),
        ("echo ${MY_VAR:-fallback}", "hello"),
        ("echo ${MISSING:-fallback}", "fallback"),
        ("echo ${EMPTY:-fallback}", "fallback"),
        ("echo [${EMPTY-fallback}]", "[]"),
        ("echo ${MY_VAR:+set}", "set"),
        ("echo [${MISSING:+set}]", "[]"),
        ("echo pre${MY_VAR}post", "prehellopost"),
        (r#"echo "${MY_VAR}""#, "hello"),
        (r#"echo "pre${MY_VAR}post""#, "prehellopost"),
        ("echo '${MY_VAR}'", "${MY_VAR}"),
        (r"echo \${MY_VAR}", "${MY_VAR}"),
        ("echo ${MISSING:-${MY_VAR}}", "hello"),
        ("echo ${MISSING:-pre${MY_VAR}post}", "prehellopost"),
    ] {
        let (code, lines) = run(script, dir.path(), &env);
        assert_eq!(code, 0, "`{script}` must exit zero");
        assert_eq!(lines, vec![(LifecycleStdio::Stdout, expected.to_string())], "`{script}`");
    }
}

/// A quoted or escaped `}` inside the expansion belongs to the default
/// value, not to the expansion, so the closing brace is the one after it.
#[test]
fn reads_past_a_quoted_closing_brace_in_a_default() {
    let dir = tempdir().expect("create a temp dir");
    let env = HashMap::new();

    let (code, lines) = run(r#"echo ${MISSING:-"}"}!"#, dir.path(), &env);
    assert_eq!(code, 0);
    assert_eq!(lines, vec![(LifecycleStdio::Stdout, "}!".to_string())]);

    let (code, lines) = run(r"echo ${MISSING:-'}'}!", dir.path(), &env);
    assert_eq!(code, 0);
    assert_eq!(lines, vec![(LifecycleStdio::Stdout, "}!".to_string())]);
}

/// Inside a double-quoted word an apostrophe is an ordinary character. It
/// neither hides the brace that closes the expansion nor opens a quoted run
/// that would keep the rest of the word from expanding.
#[test]
fn keeps_an_apostrophe_literal_inside_a_double_quoted_default() {
    let dir = tempdir().expect("create a temp dir");
    let env = HashMap::from([("MY_VAR".to_string(), "hello".to_string())]);

    let (code, lines) = run(r#"echo "${MISSING:-it's fine}""#, dir.path(), &env);
    assert_eq!(code, 0);
    assert_eq!(lines, vec![(LifecycleStdio::Stdout, "it's fine".to_string())]);

    let (code, lines) = run(r#"echo "${MISSING:-it's ${MY_VAR}}""#, dir.path(), &env);
    assert_eq!(code, 0);
    assert_eq!(lines, vec![(LifecycleStdio::Stdout, "it's hello".to_string())]);
}

/// A backslash-escaped quote is text, so it must not read as opening a quoted
/// run. It would otherwise make the rest of the script look double-quoted and
/// leave a later word's operators unescaped.
#[test]
fn keeps_an_escaped_quote_from_opening_a_quoted_run() {
    let dir = tempdir().expect("create a temp dir");
    let env = HashMap::from([("MY_VAR".to_string(), "hello".to_string())]);

    for (script, expected) in
        [(r#"echo \"${MY_VAR}\""#, r#""hello""#), (r#"echo \"${MISSING:-a;b}\""#, r#""a;b""#)]
    {
        let (code, lines) = run(script, dir.path(), &env);
        assert_eq!(code, 0, "`{script}` must exit zero");
        assert_eq!(lines, vec![(LifecycleStdio::Stdout, expected.to_string())], "`{script}`");
    }
}

/// A default may itself be a parameter expansion, to any depth, and the
/// parameter that wins still reaches the script as a `$NAME` reference.
#[test]
fn expands_a_default_that_is_itself_an_expansion() {
    let dir = tempdir().expect("create a temp dir");
    let first = ("FIRST".to_string(), "first".to_string());
    let second = ("SECOND".to_string(), "second".to_string());

    for (env, expected) in [
        (HashMap::from([first.clone(), second.clone()]), "prefirstpost"),
        (HashMap::from([second]), "presecondpost"),
        (HashMap::from([first]), "prefirstpost"),
        (HashMap::new(), "prefallbackpost"),
    ] {
        let script = "echo pre${FIRST:-${SECOND:-fallback}}post";
        let (code, lines) = run(script, dir.path(), &env);
        assert_eq!(code, 0, "`{script}` must exit zero for {env:?}");
        assert_eq!(
            lines,
            vec![(LifecycleStdio::Stdout, expected.to_string())],
            "`{script}` for {env:?}",
        );
    }
}

/// Values reach the script as word text, never as script text, so shell
/// punctuation in an environment variable stays an argument.
#[test]
fn never_runs_an_expanded_value_as_script() {
    let dir = tempdir().expect("create a temp dir");
    let env = HashMap::from([
        ("INJECTED".to_string(), "; echo pwned".to_string()),
        ("SUBSTITUTED".to_string(), "$(echo pwned)".to_string()),
    ]);

    let (code, lines) = run("echo ${INJECTED}", dir.path(), &env);
    assert_eq!(code, 0);
    assert_eq!(lines, vec![(LifecycleStdio::Stdout, "; echo pwned".to_string())]);

    let (code, lines) = run(r#"echo "${SUBSTITUTED}""#, dir.path(), &env);
    assert_eq!(code, 0);
    assert_eq!(lines, vec![(LifecycleStdio::Stdout, "$(echo pwned)".to_string())]);
}

/// POSIX makes a shell operator inside a `word` part of the word rather than
/// syntax of its own, so a default that reads like a second command is one
/// argument. Command substitution still runs, as it does in `bash`.
#[test]
fn keeps_a_shell_operator_in_a_default_as_word_text() {
    let dir = tempdir().expect("create a temp dir");
    let env = HashMap::from([("MY_VAR".to_string(), "hello".to_string())]);

    for (script, expected) in [
        ("echo [${MISSING:-safe; echo injected}]", "[safe; echo injected]"),
        (r#"echo "[${MISSING:-safe; echo injected}]""#, "[safe; echo injected]"),
        ("echo [${MISSING:-a|b}]", "[a|b]"),
        ("echo [${MISSING:-a&b}]", "[a&b]"),
        ("echo [${MISSING:-a>written.txt}]", "[a>written.txt]"),
        ("echo [${MY_VAR:+a;b}]", "[a;b]"),
        ("echo pre${MISSING:-a;b}post", "prea;bpost"),
        ("echo [${MISSING:-$(echo substituted)}]", "[substituted]"),
        // A quote inside the word ends the run the surrounding double quotes
        // opened, so the operator behind it is bare word text after all.
        (r#"echo ["${MISSING:-a"; echo pwned"}"]"#, "[a; echo pwned]"),
    ] {
        let (code, lines) = run(script, dir.path(), &env);
        assert_eq!(code, 0, "`{script}` must exit zero");
        assert_eq!(lines, vec![(LifecycleStdio::Stdout, expected.to_string())], "`{script}`");
    }

    assert!(!dir.path().join("written.txt").exists(), "a `>` in a word must not redirect");
}

/// The parser reads a run of backslashes before an operator by its own rules,
/// so an operator in a `word` is quoted rather than escaped. However many
/// backslashes the word puts in front of one, it stays text and the command
/// behind it still runs.
#[test]
fn keeps_an_operator_literal_behind_any_backslash_run() {
    let dir = tempdir().expect("create a temp dir");

    for (operator, backslashes) in [';', '&', '|', '<', '>'].into_iter().flat_map(with_run_lengths)
    {
        let run_of = r"\".repeat(backslashes);
        let script = format!("echo [${{MISSING:-a{run_of}{operator}b}}] && echo second");
        // A pair of backslashes is one literal backslash and an odd one
        // escapes the operator, so either way the operator is text.
        let literal = r"\".repeat(backslashes / 2);
        let expected = format!("[a{literal}{operator}b]");

        let (code, lines) = run(&script, dir.path(), &HashMap::new());
        assert_eq!(code, 0, "`{script}` must exit zero");
        let second = (LifecycleStdio::Stdout, "second".to_string());
        assert_eq!(lines, vec![(LifecycleStdio::Stdout, expected), second], "`{script}`");
    }
}

fn with_run_lengths(operator: char) -> impl Iterator<Item = (char, usize)> {
    (0..=3).map(move |backslashes| (operator, backslashes))
}

/// A newline in a `word` splits it, a carriage return is an ordinary
/// character, and a `#` starting a word is text. The parser would otherwise
/// end the command at the newline, split the word on the carriage return, and
/// read the `#` as opening a comment that swallows the rest of the line.
/// Every expectation here is what `bash` prints.
#[test]
fn keeps_a_newline_and_a_comment_start_in_a_default_as_word_text() {
    let dir = tempdir().expect("create a temp dir");

    for (script, expected) in [
        ("echo [${MISSING:-a\nb}] && echo second", "[a b]"),
        ("echo [${MISSING:-a\rb}] && echo second", "[a\rb]"),
        ("echo [${MISSING:-a\r\nb}] && echo second", "[a\r b]"),
        ("echo ${MISSING:-#fallback} && echo second", "#fallback"),
        ("echo [${MISSING:-#fallback}] && echo second", "[#fallback]"),
    ] {
        let (code, lines) = run(script, dir.path(), &HashMap::new());
        assert_eq!(code, 0, "`{script}` must exit zero");
        let second = (LifecycleStdio::Stdout, "second".to_string());
        assert_eq!(
            lines,
            vec![(LifecycleStdio::Stdout, expected.to_string()), second],
            "`{script}`",
        );
    }
}

/// A backslash in a `word` escapes whatever follows it, which is then the
/// plain character. Every expectation here is what `bash` prints.
#[test]
fn reads_a_backslash_in_a_default_as_escaping_what_follows() {
    let dir = tempdir().expect("create a temp dir");
    let env = HashMap::new();

    for (script, expected) in [
        (r"echo [${MISSING:-safe\; echo injected}]", "[safe; echo injected]"),
        (r"echo [${MISSING:-a\;b}]", "[a;b]"),
        (r"echo [${MISSING:-a\\;b}]", r"[a\;b]"),
        (r"echo [${MISSING:-a\$b}]", "[a$b]"),
        (r"echo [${MISSING:-a\qb}]", "[aqb]"),
        (r"echo [${MISSING:-a\ b}]", "[a b]"),
    ] {
        let (code, lines) = run(script, dir.path(), &env);
        assert_eq!(code, 0, "`{script}` must exit zero");
        assert_eq!(lines, vec![(LifecycleStdio::Stdout, expected.to_string())], "`{script}`");
    }
}

/// A `${` that never closes is scanned to the end of the script, so a script
/// of nothing but `${` would cost the square of its length. Fruitless scanning
/// draws on an allowance the whole script shares, while finding the `}` costs
/// nothing, so an expansion stays expandable however long its body is.
#[test]
fn bounds_fruitless_scanning_without_bounding_a_body() {
    let dir = tempdir().expect("create a temp dir");
    let env = HashMap::new();

    let long_body = "x".repeat(20_000);
    let (code, lines) = run(&format!("echo ${{MISSING:-{long_body}}}"), dir.path(), &env);
    assert_eq!(code, 0);
    assert_eq!(lines, vec![(LifecycleStdio::Stdout, long_body)]);

    let unclosed = "${".repeat(20_000);
    let (code, lines) = run(&format!("echo {unclosed}"), dir.path(), &env);
    assert_eq!(code, 0);
    assert_eq!(lines, vec![(LifecycleStdio::Stdout, unclosed)]);
}

/// An expansion the `$NAME` form cannot stand in for keeps the behavior it
/// has today: the emulator hands it to the script as literal text.
#[test]
fn leaves_unsupported_parameter_forms_alone() {
    let dir = tempdir().expect("create a temp dir");
    let env = HashMap::from([("MY_VAR".to_string(), "hello".to_string())]);

    for script in ["echo ${MY_VAR:=x}", "echo ${MY_VAR=x}", "echo ${#MY_VAR}", "echo ${MY_VAR"] {
        let expected = script.trim_start_matches("echo ").to_string();
        let (code, lines) = run(script, dir.path(), &env);
        assert_eq!(code, 0, "`{script}` must exit zero");
        assert_eq!(lines, vec![(LifecycleStdio::Stdout, expected)], "`{script}`");
    }
}

#[test]
fn runs_in_the_given_directory() {
    let dir = tempdir().expect("create a temp dir");
    let (code, _) = run("echo hello > written.txt", dir.path(), &HashMap::new());
    assert_eq!(code, 0);
    let written =
        std::fs::read_to_string(dir.path().join("written.txt")).expect("read the written file");
    assert_eq!(written.trim(), "hello");
}

#[test]
fn rejects_a_script_the_shell_cannot_parse() {
    let dir = tempdir().expect("create a temp dir");
    let error = execute_emulated(
        "echo 'unterminated",
        dir.path(),
        &HashMap::new(),
        EmulatedOutput::Inherit,
        None,
    )
    .expect_err("an unparsable script is an error, not an exit code");
    dbg!(&error);
    assert!(matches!(error, ShellEmulatorError::Parse { .. }));
}
