// Copyright 2018-2025 the Deno authors. MIT license.

use std::time::{
    Duration,
    Instant,
};

use deno_task_shell::{
    ExecuteResult,
    KillSignal,
    SignalKind,
};
use futures::FutureExt;

use self::test_builder::TestBuilder;

mod test_builder;

const FOLDER_SEPERATOR: char = if cfg!(windows) { '\\' } else { '/' };

#[tokio::test]
async fn comments() {
    // trailing comment after a command (denoland/deno#27644)
    TestBuilder::new()
        .command("echo foo # this is a comment")
        .assert_stdout("foo\n")
        .run()
        .await;

    // `#` with no preceding space is still a comment at a word boundary
    TestBuilder::new()
        .command("echo foo #comment")
        .assert_stdout("foo\n")
        .run()
        .await;

    // `#` in the middle of a word is a literal character, not a comment
    TestBuilder::new()
        .command("echo foo#bar")
        .assert_stdout("foo#bar\n")
        .run()
        .await;

    // `#` inside quotes is literal
    TestBuilder::new()
        .command("echo '# not a comment'")
        .assert_stdout("# not a comment\n")
        .run()
        .await;

    // comment lines interspersed with commands
    TestBuilder::new()
        .command("# leading\necho foo\n# middle\necho bar\n# trailing")
        .assert_stdout("foo\nbar\n")
        .run()
        .await;

    // comment after a `&&` continuation
    TestBuilder::new()
        .command("echo foo && # comment\necho bar")
        .assert_stdout("foo\nbar\n")
        .run()
        .await;
}

#[tokio::test]
async fn commands() {
    TestBuilder::new()
        .command("echo 1")
        .assert_stdout("1\n")
        .run()
        .await;

    TestBuilder::new()
        .command("echo 1 2   3")
        .assert_stdout("1 2 3\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#"echo "1 2   3""#)
        .assert_stdout("1 2   3\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r"echo 1 2\ \ \ 3")
        .assert_stdout("1 2   3\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#"echo "1 2\ \ \ 3""#)
        .assert_stdout("1 2\\ \\ \\ 3\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#"echo test$(echo "1    2")"#)
        .assert_stdout("test1 2\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#"TEST="1   2" ; echo $TEST"#)
        .assert_stdout("1 2\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#"VAR=1 deno eval 'console.log(Deno.env.get("VAR"))' && echo $VAR"#)
        .assert_stdout("1\n\n")
        .run()
        .await;

    TestBuilder::new()
        .command(
            r#"VAR=1 VAR2=2 deno eval 'console.log(Deno.env.get("VAR") + Deno.env.get("VAR2"))'"#,
        )
        .assert_stdout("12\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#"EMPTY= deno eval 'console.log(`EMPTY: ${Deno.env.get("EMPTY")}`)'"#)
        .assert_stdout("EMPTY: \n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#""echo" "1""#)
        .assert_stdout("1\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#""echo" "*""#)
        .assert_stdout("*\n")
        .run()
        .await;

    TestBuilder::new()
        .command("echo test-dashes")
        .assert_stdout("test-dashes\n")
        .run()
        .await;

    TestBuilder::new()
        .command("echo 'a/b'/c")
        .assert_stdout("a/b/c\n")
        .run()
        .await;

    TestBuilder::new()
        .command("echo 'a/b'ctest\"te  st\"'asdf'")
        .assert_stdout("a/bctestte  stasdf\n")
        .run()
        .await;

    TestBuilder::new()
    .command("echo --test=\"2\" --test='2' test\"TEST\" TEST'test'TEST 'test''test' test'test'\"test\" \"test\"\"test\"'test'")
    .assert_stdout("--test=2 --test=2 testTEST TESTtestTEST testtest testtesttest testtesttest\n")
    .run()
    .await;

    TestBuilder::new()
        .command("deno eval 'console.log(1)'")
        .env_var("PATH", "")
        .assert_stderr("deno: command not found\n")
        .assert_exit_code(127)
        .run()
        .await;

    TestBuilder::new().command("unset").run().await;
}

#[tokio::test]
async fn boolean_logic() {
    TestBuilder::new()
        .command("echo 1 && echo 2 || echo 3")
        .assert_stdout("1\n2\n")
        .run()
        .await;

    TestBuilder::new()
        .command("echo 1 || echo 2 && echo 3")
        .assert_stdout("1\n3\n")
        .run()
        .await;

    TestBuilder::new()
        .command("echo 1 || (echo 2 && echo 3)")
        .assert_stdout("1\n")
        .run()
        .await;

    TestBuilder::new()
        .command("false || false || (echo 2 && false) || echo 3")
        .assert_stdout("2\n3\n")
        .run()
        .await;
}

#[tokio::test]
async fn null_command() {
    // POSIX `:` is the null command — always succeeds, discards args.
    TestBuilder::new().command(":").run().await;

    TestBuilder::new()
        .command(": ignored args && echo ok")
        .assert_stdout("ok\n")
        .run()
        .await;

    TestBuilder::new()
        .command(": && echo yes || echo no")
        .assert_stdout("yes\n")
        .run()
        .await;
}

#[tokio::test]
async fn exit() {
    TestBuilder::new()
        .command("exit 1")
        .assert_exit_code(1)
        .run()
        .await;

    TestBuilder::new()
        .command("exit 5")
        .assert_exit_code(5)
        .run()
        .await;

    TestBuilder::new()
        .command("exit 258 && echo 1")
        .assert_exit_code(2)
        .run()
        .await;

    TestBuilder::new()
        .command("(exit 0) && echo 1")
        .assert_stdout("1\n")
        .run()
        .await;

    TestBuilder::new()
        .command("(exit 1) && echo 1")
        .assert_exit_code(1)
        .run()
        .await;

    TestBuilder::new()
        .command("echo 1 && (exit 1)")
        .assert_stdout("1\n")
        .assert_exit_code(1)
        .run()
        .await;

    TestBuilder::new()
        .command("exit ; echo 2")
        .assert_exit_code(1)
        .run()
        .await;

    TestBuilder::new()
        .command("exit bad args")
        .assert_stderr("exit: too many arguments\n")
        .assert_exit_code(2)
        .run()
        .await;
}

#[tokio::test]
async fn brace_expansion() {
    // comma form
    TestBuilder::new()
        .command("echo {a,b,c}")
        .assert_stdout("a b c\n")
        .run()
        .await;

    // prefix and suffix concat
    TestBuilder::new()
        .command("echo pre{a,b}post")
        .assert_stdout("preapost prebpost\n")
        .run()
        .await;

    // cartesian product across multiple braces
    TestBuilder::new()
        .command("echo {a,b}{1,2}")
        .assert_stdout("a1 a2 b1 b2\n")
        .run()
        .await;

    // nested
    TestBuilder::new()
        .command("echo {a,{b,c}}")
        .assert_stdout("a b c\n")
        .run()
        .await;

    // integer range
    TestBuilder::new()
        .command("echo {1..3}")
        .assert_stdout("1 2 3\n")
        .run()
        .await;

    // reverse range
    TestBuilder::new()
        .command("echo {3..1}")
        .assert_stdout("3 2 1\n")
        .run()
        .await;

    // integer range with step
    TestBuilder::new()
        .command("echo {1..7..2}")
        .assert_stdout("1 3 5 7\n")
        .run()
        .await;

    // single-char range
    TestBuilder::new()
        .command("echo {a..e}")
        .assert_stdout("a b c d e\n")
        .run()
        .await;

    // bare {} is literal (find -exec {} idiom — denoland/deno#20893)
    TestBuilder::new()
        .command("echo {}")
        .assert_stdout("{}\n")
        .run()
        .await;

    // single-element brace is literal (no comma, no range)
    TestBuilder::new()
        .command("echo {abc}")
        .assert_stdout("{abc}\n")
        .run()
        .await;

    // unmatched `{` is literal
    TestBuilder::new()
        .command("echo {abc")
        .assert_stdout("{abc\n")
        .run()
        .await;

    // unmatched `}` is literal
    TestBuilder::new()
        .command("echo abc}")
        .assert_stdout("abc}\n")
        .run()
        .await;

    // inside double quotes braces are literal
    TestBuilder::new()
        .command(r#"echo "{a,b}""#)
        .assert_stdout("{a,b}\n")
        .run()
        .await;

    // inside single quotes braces are literal
    TestBuilder::new()
        .command("echo '{a,b}'")
        .assert_stdout("{a,b}\n")
        .run()
        .await;

    // empty alternatives expand to empty strings
    TestBuilder::new()
        .command("echo x{a,,b}y")
        .assert_stdout("xay xy xby\n")
        .run()
        .await;
}

#[tokio::test]
async fn double_quotes() {
    // escaped
    TestBuilder::new()
        .command("echo \"testing\\\" this out\"")
        .assert_stdout("testing\" this out\n")
        .run()
        .await;
}

#[tokio::test]
async fn escaped_operators() {
    // a backslash before an operator makes it part of the word
    // (denoland/deno#36401)
    TestBuilder::new()
        .command(r"echo one \; two")
        .assert_stdout("one ; two\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r"echo a\&b \| c \> d")
        .assert_stdout("a&b | c > d\n")
        .run()
        .await;

    // still literal within quotes
    TestBuilder::new()
        .command(r#"echo "one \; two""#)
        .assert_stdout("one \\; two\n")
        .run()
        .await;

    // `\\` before an operator is a literal backslash and the operator still
    // separates, which keeps a trailing Windows separator writable unquoted
    TestBuilder::new()
        .command(r"echo one\\;echo two")
        .assert_stdout("one\\\\\ntwo\n")
        .run()
        .await;
}

#[tokio::test]
async fn async_commands() {
    TestBuilder::new()
        .command("sleep 0.1 && echo 2 & echo 1")
        .assert_stdout("1\n2\n")
        .run()
        .await;

    TestBuilder::new()
        .command("(sleep 0.1 && echo 2 &) ; echo 1")
        .assert_stdout("1\n2\n")
        .run()
        .await;

    TestBuilder::new()
        .command("(sleep 0.1 && echo 2) & echo 1")
        .assert_stdout("1\n2\n")
        .run()
        .await;

    TestBuilder::new()
        .command("$(sleep 0.1 && echo 1 & $(sleep 0.2 && echo 2 & echo echo) & echo echo)")
        .assert_stdout("1 2\n")
        .run()
        .await;

    TestBuilder::new()
        .command("exit 1 & exit 0")
        .assert_exit_code(1)
        .run()
        .await;

    // should not output because the `exit 1` will cancel the sleep
    TestBuilder::new()
        .command("sleep 5 && echo 1 & exit 1")
        .assert_exit_code(1)
        .run()
        .await;

    // should fail when async command exits
    TestBuilder::new()
        .command("exit 1 & exit 0")
        .assert_exit_code(1)
        .run()
        .await;

    // should fail when async command fails and cancel any running command
    TestBuilder::new()
        .command("deno eval 'Deno.exit(1)' & sleep 5 && echo 2 & echo 1")
        .assert_stdout("1\n")
        .assert_exit_code(1)
        .run()
        .await;

    // should cancel running command
    TestBuilder::new()
    .command("sleep 10 & sleep 0.5 && exit 2 & deno eval 'console.log(1); setTimeout(() => { console.log(3) }, 10_000);'")
    .assert_stdout("1\n")
    .assert_exit_code(2)
    .run()
    .await;

    // should be able to opt out by doing an `|| exit 0`
    TestBuilder::new()
        .command("deno eval 'Deno.exit(1)' || exit 0 & echo 1")
        .assert_stdout("1\n")
        .assert_exit_code(0)
        .run()
        .await;
}

#[tokio::test]
async fn command_substitution() {
    TestBuilder::new()
        .command("echo $(echo 1)")
        .assert_stdout("1\n")
        .run()
        .await;

    TestBuilder::new()
        .command("echo $(echo 1 && echo 2)")
        .assert_stdout("1 2\n")
        .run()
        .await;

    TestBuilder::new()
        .command("echo \"hi $(echo 1)\"")
        .assert_stdout("hi 1\n")
        .run()
        .await;

    // nested
    TestBuilder::new()
        .command("echo $(echo $(echo 1))")
        .assert_stdout("1\n")
        .run()
        .await;

    // async inside subshell should wait
    TestBuilder::new()
        .command("$(sleep 0.1 && echo 1 & echo echo) 2")
        .assert_stdout("1 2\n")
        .run()
        .await;
    TestBuilder::new()
        .command("$(sleep 0.1 && echo 1 && exit 5 &) ; echo 2")
        .assert_stdout("2\n")
        .assert_stderr("1: command not found\n")
        .run()
        .await;
}

#[tokio::test]
async fn backticks() {
    TestBuilder::new()
        .command("echo ``")
        .assert_stdout("\n")
        .run()
        .await;

    TestBuilder::new()
        .command("echo `echo 1`")
        .assert_stdout("1\n")
        .run()
        .await;

    TestBuilder::new()
        .command("echo `echo 1 && echo 2`")
        .assert_stdout("1 2\n")
        .run()
        .await;

    TestBuilder::new()
        .command("echo \"hi `echo 1`\"")
        .assert_stdout("hi 1\n")
        .run()
        .await;

    // nested
    TestBuilder::new()
        .command("echo `echo \\`echo 1\\``")
        .assert_stdout("1\n")
        .run()
        .await;

    // async inside subshell should wait
    TestBuilder::new()
        .command("`sleep 0.1 && echo 1 & echo echo` 2")
        .assert_stdout("1 2\n")
        .run()
        .await;
    TestBuilder::new()
        .command("`sleep 0.1 && echo 1 && exit 5 &` ; echo 2")
        .assert_stdout("2\n")
        .assert_stderr("1: command not found\n")
        .run()
        .await;
}

#[tokio::test]
async fn shell_variables() {
    TestBuilder::new()
        .command(
            r#"echo $VAR && VAR=1 && echo $VAR && deno eval 'console.log(Deno.env.get("VAR"))'"#,
        )
        .assert_stdout("\n1\nundefined\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#"VAR=1 && echo $VAR$VAR"#)
        .assert_stdout("11\n")
        .run()
        .await;

    TestBuilder::new()
    .command(r#"VAR=1 && echo Test$VAR && echo $(echo "Test: $VAR") ; echo CommandSub$($VAR); echo $ ; echo \$VAR"#)
    .assert_stdout("Test1\nTest: 1\nCommandSub\n$\n$VAR\n")
    .assert_stderr("1: command not found\n")
    .run()
    .await;
}

#[tokio::test]
async fn env_variables() {
    TestBuilder::new()
    .command(r#"echo $VAR && export VAR=1 && echo $VAR && deno eval 'console.log(Deno.env.get("VAR"))'"#)
    .assert_stdout("\n1\n1\n")
    .run()
    .await;

    TestBuilder::new()
        .command(r#"export VAR=1 VAR2=testing VAR3="test this out" && echo $VAR $VAR2 $VAR3"#)
        .assert_stdout("1 testing test this out\n")
        .run()
        .await;
}

#[tokio::test]
async fn exit_code_var() {
    TestBuilder::new()
        .command(r#"echo $? ; echo $? ; false ; echo $?"#)
        .assert_stdout("\n0\n1\n")
        .run()
        .await;
    TestBuilder::new()
        .command(r#"(false || echo $?) && echo $?"#)
        .assert_stdout("1\n0\n")
        .run()
        .await;
    TestBuilder::new()
        .command(r#"! false && echo $?"#)
        .assert_stdout("0\n")
        .run()
        .await;
    TestBuilder::new()
        .command(r#"(deno eval 'Deno.exit(25)') || echo $?"#)
        .assert_stdout("25\n")
        .run()
        .await;
}

#[tokio::test]
async fn sequential_lists() {
    TestBuilder::new()
        .command(r#"echo 1 ; sleep 0.1 && echo 4 & echo 2 ; echo 3;"#)
        .assert_stdout("1\n2\n3\n4\n")
        .run()
        .await;
}

#[tokio::test]
async fn pipeline() {
    TestBuilder::new()
        .command(r#"echo 1 | deno eval 'await Deno.stdin.readable.pipeTo(Deno.stdout.writable)'"#)
        .assert_stdout("1\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#"echo 1 | echo 2 && echo 3"#)
        .assert_stdout("2\n3\n")
        .run()
        .await;

    TestBuilder::new()
    .command(r#"echo $(sleep 0.1 && echo 2 & echo 1) | deno eval 'await Deno.stdin.readable.pipeTo(Deno.stdout.writable)'"#)
    .assert_stdout("1 2\n")
    .run()
    .await;

    TestBuilder::new()
    .command(r#"echo 2 | echo 1 | deno eval 'await Deno.stdin.readable.pipeTo(Deno.stdout.writable)'"#)
    .assert_stdout("1\n")
    .run()
    .await;

    TestBuilder::new()
    .command(r#"deno eval 'console.log(1); console.error(2);' | deno eval 'await Deno.stdin.readable.pipeTo(Deno.stdout.writable)'"#)
    .assert_stdout("1\n")
    .assert_stderr("2\n")
    .run()
    .await;

    // stdout and stderr pipeline

    TestBuilder::new()
    .command(r#"deno eval 'console.log(1); console.error(2);' |& deno eval 'await Deno.stdin.readable.pipeTo(Deno.stdout.writable)'"#)
    .assert_stdout("1\n2\n")
    .run()
    .await;

    TestBuilder::new()
    // add bit of a delay while outputting stdout so that it doesn't race with stderr
    .command(r#"deno eval 'console.log(1); console.error(2);' | deno eval 'setTimeout(async () => { await Deno.stdin.readable.pipeTo(Deno.stderr.writable) }, 10)' |& deno eval 'await Deno.stdin.readable.pipeTo(Deno.stderr.writable)'"#)
    // still outputs 2 because the first command didn't pipe stderr
    .assert_stderr("2\n1\n")
    .run()
    .await;

    // |& pipeline should still pipe stdout
    TestBuilder::new()
        .command(r#"echo 1 |& deno eval 'await Deno.stdin.readable.pipeTo(Deno.stdout.writable)'"#)
        .assert_stdout("1\n")
        .run()
        .await;

    // pipeline with redirect
    TestBuilder::new()
    .command(r#"echo 1 | deno eval 'await Deno.stdin.readable.pipeTo(Deno.stdout.writable)' > output.txt"#)
    .assert_file_equals("output.txt", "1\n")
    .run()
    .await;

    // pipeline with stderr redirect
    TestBuilder::new()
    .command(r#"echo 1 | deno eval 'await Deno.stdin.readable.pipeTo(Deno.stderr.writable)' 2> output.txt"#)
    .assert_file_equals("output.txt", "1\n")
    .run()
    .await;

    // redirect on the left side of a pipe: the left command's stdout goes to the
    // file, so the right side of the pipe receives nothing.
    TestBuilder::new()
    .command(r#"echo 1 > output.txt | deno eval 'const s = await new Response(Deno.stdin.readable).text(); console.log("len=" + s.length)'"#)
    .assert_file_equals("output.txt", "1\n")
    .assert_stdout("len=0\n")
    .run()
    .await;

    // `2>&1 | reader` — both stdout and stderr of the left command are
    // funneled through the pipe to the reader. This is the use case from
    // denoland/deno#28808.
    TestBuilder::new()
    .command(r#"deno eval 'console.log("o"); console.error("e")' 2>&1 | deno eval 'await Deno.stdin.readable.pipeTo(Deno.stdout.writable)'"#)
    .assert_stdout("o\ne\n")
    .run()
    .await;
}

#[tokio::test]
async fn negated() {
    TestBuilder::new()
        .command(r#"! echo 1 && echo 2"#)
        .assert_stdout("1\n")
        .assert_exit_code(1)
        .run()
        .await;
    TestBuilder::new()
        .command(r#"! echo 1 || echo 2"#)
        .assert_stdout("1\n2\n")
        .run()
        .await;
    TestBuilder::new()
        .command(r#"! (echo 1 | echo 2 && echo 3) || echo 4"#)
        .assert_stdout("2\n3\n4\n")
        .run()
        .await;
    TestBuilder::new()
        .command(r#"! echo 1 | echo 2 && echo 3"#)
        .assert_stdout("2\n")
        .assert_exit_code(1)
        .run()
        .await;
    TestBuilder::new()
        .command(r#"! (exit 5) && echo 1"#)
        .assert_stdout("1\n")
        .run()
        .await;
    TestBuilder::new()
        .command(r#"! exit 5 && echo 1"#)
        .assert_exit_code(5)
        .run()
        .await;
    TestBuilder::new()
        .command(r#"! echo 1 && echo 2 &"#)
        .assert_stdout("1\n")
        // differing behaviour to shells, where this async command will actually fail
        .assert_exit_code(1)
        .run()
        .await;

    // test no spaces
    TestBuilder::new()
    .command(r#"!echo 1 && echo 2"#)
    .assert_stderr("History expansion is not supported:\n  !echo\n  ~\n\nPerhaps you meant to add a space after the exclamation point to negate the command?\n  ! echo\n")
    .assert_exit_code(1)
    .run()
    .await;
}

#[tokio::test]
async fn redirects_output() {
    TestBuilder::new()
        .command(r#"echo 5 6 7 > test.txt"#)
        .assert_file_equals("test.txt", "5 6 7\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#"echo 1 2 3 && echo 1 > test.txt"#)
        .assert_stdout("1 2 3\n")
        .assert_file_equals("test.txt", "1\n")
        .run()
        .await;

    // subdir
    TestBuilder::new()
        .command(r#"mkdir subdir && cd subdir && echo 1 2 3 > test.txt"#)
        .assert_file_equals("subdir/test.txt", "1 2 3\n")
        .run()
        .await;

    // absolute path
    TestBuilder::new()
        .command(r#"echo 1 2 3 > "$PWD/test.txt""#)
        .assert_file_equals("test.txt", "1 2 3\n")
        .run()
        .await;

    // stdout
    TestBuilder::new()
        .command(r#"deno eval 'console.log(1); console.error(5)' 1> test.txt"#)
        .assert_stderr("5\n")
        .assert_file_equals("test.txt", "1\n")
        .run()
        .await;

    // stderr
    TestBuilder::new()
        .command(r#"deno eval 'console.log(1); console.error(5)' 2> test.txt"#)
        .assert_stdout("1\n")
        .assert_file_equals("test.txt", "5\n")
        .run()
        .await;

    // invalid fd
    TestBuilder::new()
        .command(r#"echo 2 3> test.txt"#)
        .ensure_temp_dir()
        .assert_stderr("only redirecting to stdout (1) and stderr (2) is supported\n")
        .assert_exit_code(1)
        .run()
        .await;

    // /dev/null
    TestBuilder::new()
        .command(r#"deno eval 'console.log(1); console.error(5)' 2> /dev/null"#)
        .assert_stdout("1\n")
        .run()
        .await;

    // appending
    TestBuilder::new()
        .command(r#"echo 1 > test.txt && echo 2 >> test.txt"#)
        .assert_file_equals("test.txt", "1\n2\n")
        .run()
        .await;

    // &> and &>> redirect
    TestBuilder::new()
        .command(concat!(
            "deno eval 'console.log(1); setTimeout(() => console.error(23), 10)' &> file.txt &&",
            "deno eval 'console.log(456); setTimeout(() => console.error(789), 10)' &>> file.txt"
        ))
        .assert_file_equals("file.txt", "1\n23\n456\n789\n")
        .run()
        .await;

    // multiple arguments after re-direct
    TestBuilder::new()
        .command(r"export TwoArgs=testing\ this && echo 1 > $TwoArgs")
        .assert_stderr(concat!(
            "redirect path must be 1 argument, but found 2 ",
            "(testing this). Did you mean to quote it (ex. \"testing this\")?\n"
        ))
        .assert_exit_code(1)
        .run()
        .await;

    // zero arguments after re-direct
    TestBuilder::new()
        .command(r#"echo 1 > $EMPTY"#)
        .assert_stderr("redirect path must be 1 argument, but found 0\n")
        .assert_exit_code(1)
        .run()
        .await;

    TestBuilder::new()
    .command(r#"echo 1 >&3"#)
    .assert_stderr(
      "deno_task_shell: output redirecting file descriptors beyond stdout and stderr is not implemented\n",
    )
    .assert_exit_code(1)
    .run()
    .await;

    TestBuilder::new()
        .command(r#"echo 1 >&1"#)
        .assert_stdout("1\n")
        .assert_exit_code(0)
        .run()
        .await;

    TestBuilder::new()
        .command(r#"echo 1 >&2"#)
        .assert_stderr("1\n")
        .assert_exit_code(0)
        .run()
        .await;

    TestBuilder::new()
        .command(r#"deno eval 'console.error(2)' 2>&1"#)
        .assert_stdout("2\n")
        .assert_exit_code(0)
        .run()
        .await;
}

#[tokio::test]
async fn redirects_input() {
    TestBuilder::new()
        .file("test.txt", "Hi!")
        .command(r#"cat - < test.txt"#)
        .assert_stdout("Hi!")
        .run()
        .await;

    TestBuilder::new()
        .file("test.txt", "Hi!\n")
        .command(r#"cat - < test.txt && echo There"#)
        .assert_stdout("Hi!\nThere\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#"cat - <&0"#)
        .assert_stderr("deno_task_shell: input redirecting file descriptors is not implemented\n")
        .assert_exit_code(1)
        .run()
        .await;
}

#[tokio::test]
async fn pwd() {
    TestBuilder::new()
        .directory("sub_dir")
        .file("file.txt", "test")
        .command("pwd && cd sub_dir && pwd && cd ../ && pwd")
        // the actual temp directory will get replaced here
        .assert_stdout(&format!("$TEMP_DIR\n$TEMP_DIR{FOLDER_SEPERATOR}sub_dir\n$TEMP_DIR\n"))
        .run()
        .await;

    TestBuilder::new()
        .command("pwd -M")
        .assert_stderr("pwd: unsupported flag: -M\n")
        .assert_exit_code(1)
        .run()
        .await;
}

#[tokio::test]
async fn subshells() {
    TestBuilder::new()
        .command("(export TEST=1) && echo $TEST")
        .assert_stdout("\n")
        .assert_exit_code(0)
        .run()
        .await;
    TestBuilder::new()
        .directory("sub_dir")
        .command("echo $PWD && (cd sub_dir && echo $PWD) && echo $PWD")
        .assert_stdout(&format!("$TEMP_DIR\n$TEMP_DIR{FOLDER_SEPERATOR}sub_dir\n$TEMP_DIR\n"))
        .assert_exit_code(0)
        .run()
        .await;
    TestBuilder::new()
        .command("export TEST=1 && (echo $TEST && unset TEST && echo $TEST) && echo $TEST")
        .assert_stdout("1\n\n1\n")
        .assert_exit_code(0)
        .run()
        .await;
    TestBuilder::new()
        .command("(exit 1) && echo 1")
        .assert_exit_code(1)
        .run()
        .await;
    TestBuilder::new()
        .command("(exit 1) || echo 1")
        .assert_stdout("1\n")
        .assert_exit_code(0)
        .run()
        .await;
}

#[tokio::test]
#[cfg(unix)]
async fn pwd_logical() {
    TestBuilder::new()
        .directory("main")
        .command("ln -s main symlinked_main && cd symlinked_main && pwd && pwd -L")
        .assert_stdout("$TEMP_DIR/symlinked_main\n$TEMP_DIR/main\n")
        .run()
        .await;
}

#[tokio::test]
async fn cat() {
    // no args
    TestBuilder::new()
        .command("cat")
        .stdin("hello")
        .assert_stdout("hello")
        .run()
        .await;

    // dash
    TestBuilder::new()
        .command("cat -")
        .stdin("hello")
        .assert_stdout("hello")
        .run()
        .await;

    // file
    TestBuilder::new()
        .command("cat file")
        .file("file", "test")
        .assert_stdout("test")
        .run()
        .await;

    // multiple files
    TestBuilder::new()
        .command("cat file1 file2")
        .file("file1", "test")
        .file("file2", "other")
        .assert_stdout("testother")
        .run()
        .await;

    // multiple files and stdin
    TestBuilder::new()
        .command("cat file1 file2 -")
        .file("file1", "test\n")
        .file("file2", "other\n")
        .stdin("hello")
        .assert_stdout("test\nother\nhello")
        .run()
        .await;

    // multiple files and stdin different order
    TestBuilder::new()
        .command("cat file1 - file2")
        .file("file1", "test\n")
        .file("file2", "other\n")
        .stdin("hello\n")
        .assert_stdout("test\nhello\nother\n")
        .run()
        .await;

    // file containing a command to evaluate
    TestBuilder::new()
        .command("$(cat file)")
        .file("file", "echo hello")
        .assert_stdout("hello\n")
        .run()
        .await;
}

#[tokio::test]
async fn head() {
    // no args
    TestBuilder::new()
        .command("head")
        .stdin("foo\nbar\nbaz\nqux\nquuux\ncorge\ngrault\ngarply\nwaldo\nfred\nplugh\n")
        .assert_stdout("foo\nbar\nbaz\nqux\nquuux\ncorge\ngrault\ngarply\nwaldo\nfred\n")
        .run()
        .await;

    // dash
    TestBuilder::new()
        .command("head -")
        .stdin("foo\nbar\nbaz\nqux\nquuux\ncorge\ngrault\ngarply\nwaldo\nfred\nplugh\n")
        .assert_stdout("foo\nbar\nbaz\nqux\nquuux\ncorge\ngrault\ngarply\nwaldo\nfred\n")
        .run()
        .await;

    // file
    TestBuilder::new()
        .command("head file")
        .file("file", "foo\nbar\nbaz\nqux\nquuux\ncorge\ngrault\ngarply\nwaldo\nfred\nplugh\n")
        .assert_stdout("foo\nbar\nbaz\nqux\nquuux\ncorge\ngrault\ngarply\nwaldo\nfred\n")
        .run()
        .await;

    // dash + longer than internal buffer (512)
    TestBuilder::new()
        .command("head -")
        .stdin(
            "foo\nbar\nbaz\nqux\nquuux\ncorge\ngrault\ngarply\nwaldo\nfred\nplugh\n"
                .repeat(10)
                .as_str(),
        )
        .assert_stdout("foo\nbar\nbaz\nqux\nquuux\ncorge\ngrault\ngarply\nwaldo\nfred\n")
        .run()
        .await;

    // file + longer than internal buffer (512)
    TestBuilder::new()
        .command("head file")
        .file(
            "file",
            "foo\nbar\nbaz\nqux\nquuux\ncorge\ngrault\ngarply\nwaldo\nfred\nplugh\n"
                .repeat(1024)
                .as_str(),
        )
        .assert_stdout("foo\nbar\nbaz\nqux\nquuux\ncorge\ngrault\ngarply\nwaldo\nfred\n")
        .run()
        .await;

    // shorter than 10 lines
    TestBuilder::new()
        .command("head")
        .stdin("foo\nbar")
        .assert_stdout("foo\nbar")
        .run()
        .await;

    // -n
    TestBuilder::new()
        .command("head -n 2")
        .stdin("foo\nbar\nbaz\nqux\nquuux")
        .assert_stdout("foo\nbar\n")
        .run()
        .await;

    // --lines
    TestBuilder::new()
        .command("head --lines=3")
        .stdin("foo\nbar\nbaz\nqux\nquuux")
        .assert_stdout("foo\nbar\nbaz\n")
        .run()
        .await;

    // negative line count: -n -1 (all but last 1 line)
    TestBuilder::new()
        .command("head -n -1")
        .stdin("line1\nline2\nline3\nline4\nline5\n")
        .assert_stdout("line1\nline2\nline3\nline4\n")
        .run()
        .await;

    // negative line count: -n -2 (all but last 2 lines)
    TestBuilder::new()
        .command("head -n -2")
        .stdin("line1\nline2\nline3\nline4\nline5\n")
        .assert_stdout("line1\nline2\nline3\n")
        .run()
        .await;

    // negative line count with --lines
    TestBuilder::new()
        .command("head --lines=-3")
        .stdin("line1\nline2\nline3\nline4\nline5\n")
        .assert_stdout("line1\nline2\n")
        .run()
        .await;

    // negative line count with file
    TestBuilder::new()
        .command("head -n -1 file")
        .file("file", "line1\nline2\nline3\nline4\nline5\n")
        .assert_stdout("line1\nline2\nline3\nline4\n")
        .run()
        .await;

    // negative line count where skip >= total lines (empty output)
    TestBuilder::new()
        .command("head -n -10")
        .stdin("line1\nline2\nline3\n")
        .assert_stdout("")
        .run()
        .await;
}

// Basic integration tests as there are unit tests in the commands
#[tokio::test]
async fn mv() {
    // single file
    TestBuilder::new()
        .command("mv file1.txt file2.txt")
        .file("file1.txt", "test")
        .assert_not_exists("file1.txt")
        .assert_exists("file2.txt")
        .run()
        .await;

    // multiple files to folder
    TestBuilder::new()
        .command("mkdir sub_dir && mv file1.txt file2.txt sub_dir")
        .file("file1.txt", "test1")
        .file("file2.txt", "test2")
        .assert_not_exists("file1.txt")
        .assert_not_exists("file2.txt")
        .assert_exists("sub_dir/file1.txt")
        .assert_exists("sub_dir/file2.txt")
        .run()
        .await;

    // error message
    TestBuilder::new()
        .command("mv file1.txt file2.txt")
        .assert_exit_code(1)
        .assert_stderr(&format!(
            "mv: could not move file1.txt to file2.txt: {}\n",
            no_such_file_error_text()
        ))
        .run()
        .await;
}

// Basic integration tests as there are unit tests in the commands
#[tokio::test]
async fn cp() {
    // single file
    TestBuilder::new()
        .command("cp file1.txt file2.txt")
        .file("file1.txt", "test")
        .assert_exists("file1.txt")
        .assert_exists("file2.txt")
        .run()
        .await;

    // multiple files to folder
    TestBuilder::new()
        .command("mkdir sub_dir && cp file1.txt file2.txt sub_dir")
        .file("file1.txt", "test1")
        .file("file2.txt", "test2")
        .assert_exists("file1.txt")
        .assert_exists("file2.txt")
        .assert_exists("sub_dir/file1.txt")
        .assert_exists("sub_dir/file2.txt")
        .run()
        .await;

    // error message
    TestBuilder::new()
        .command("cp file1.txt file2.txt")
        .assert_exit_code(1)
        .assert_stderr(&format!(
            "cp: could not copy file1.txt to file2.txt: {}\n",
            no_such_file_error_text()
        ))
        .run()
        .await;
}

// Basic integration tests as there are unit tests in the commands
#[tokio::test]
async fn mkdir() {
    TestBuilder::new()
        .command("mkdir sub_dir")
        .assert_exists("sub_dir")
        .run()
        .await;

    // error message
    TestBuilder::new()
        .command("mkdir file.txt")
        .file("file.txt", "test")
        .assert_stderr("mkdir: cannot create directory 'file.txt': File exists\n")
        .assert_exit_code(1)
        .run()
        .await;
}

// Basic integration tests as there are unit tests in the commands
#[tokio::test]
async fn rm() {
    TestBuilder::new()
        .command("mkdir sub_dir && rm -d sub_dir && rm file.txt")
        .file("file.txt", "")
        .assert_not_exists("sub_dir")
        .assert_not_exists("file.txt")
        .run()
        .await;

    // error message
    TestBuilder::new()
        .command("rm file.txt")
        .assert_stderr(&format!("rm: cannot remove 'file.txt': {}\n", no_such_file_error_text()))
        .assert_exit_code(1)
        .run()
        .await;

    // rm -f should ignore non-existent files
    TestBuilder::new()
        .command("rm -f nonexistent.txt")
        .assert_exit_code(0)
        .run()
        .await;

    // rm -rf should ignore non-existent directories
    TestBuilder::new()
        .command("rm -rf nonexistent_dir")
        .assert_exit_code(0)
        .run()
        .await;

    // rm -rf with glob pattern that matches nothing should succeed
    // (when failglob is disabled, the pattern is passed literally to rm)
    TestBuilder::new()
        .command("rm -rf *.nonexistent")
        .assert_exit_code(0)
        .run()
        .await;
}

// Basic integration tests as there are unit tests in the commands
#[tokio::test]
async fn unset() {
    // Unset 1 shell variable
    TestBuilder::new()
        .command(r#"VAR1=1 && VAR2=2 && VAR3=3 && unset VAR1 && echo $VAR1 $VAR2 $VAR3"#)
        .assert_stdout("2 3\n")
        .run()
        .await;

    // Unset 1 env variable
    TestBuilder::new()
    .command(r#"export VAR1=1 VAR2=2 VAR3=3 && unset VAR1 && deno eval 'for (let i = 1; i <= 3; i++) { const val = Deno.env.get(`VAR${i}`); console.log(val); }'"#)
    .assert_stdout("undefined\n2\n3\n")
    .run()
    .await;

    // Unset 2 shell variables
    TestBuilder::new()
        .command(r#"VAR1=1 && VAR2=2 && VAR3=3 && unset VAR1 VAR2 && echo $VAR1 $VAR2 $VAR3"#)
        .assert_stdout("3\n")
        .run()
        .await;

    // Unset 2 env variables
    TestBuilder::new()
    .command(r#"export VAR1=1 VAR2=2 VAR3=3 && unset VAR1 VAR2 && deno eval 'for (let i = 1; i <= 3; i++) { const val = Deno.env.get(`VAR${i}`); console.log(val); }'"#)
    .assert_stdout("undefined\nundefined\n3\n")
    .run()
    .await;

    // Unset 2 shell variables with -v enabled
    TestBuilder::new()
        .command(r#"VAR1=1 && VAR2=2 && VAR3=3 && unset -v VAR1 VAR2 && echo $VAR1 $VAR2 $VAR3"#)
        .assert_stdout("3\n")
        .run()
        .await;

    // Unset 1 env variable with -v enabled
    TestBuilder::new()
    .command(r#"export VAR1=1 VAR2=2 VAR3=3 && unset -v VAR1 && deno eval 'for (let i = 1; i <= 3; i++) { const val = Deno.env.get(`VAR${i}`); console.log(val); }'"#)
    .assert_stdout("undefined\n2\n3\n")
    .run()
    .await;

    // Unset 2 env variables with -v enabled
    TestBuilder::new()
    .command(r#"export VAR1=1 VAR2=2 VAR3=3 && unset -v VAR1 VAR2 && deno eval 'for (let i = 1; i <= 3; i++) { const val = Deno.env.get(`VAR${i}`); console.log(val); }'"#)
    .assert_stdout("undefined\nundefined\n3\n")
    .run()
    .await;

    // Unset 1 shell variable and 1 env variable at the same time
    TestBuilder::new()
        .command(r#"VAR=1 && export ENV_VAR=2 && unset VAR ENV_VAR && echo $VAR $ENV_VAR"#)
        .assert_stdout("\n")
        .run()
        .await;

    // -f is not supported
    TestBuilder::new()
        .command(r#"export VAR=42 && unset -f VAR"#)
        .assert_stderr("unset: unsupported flag: -f\n")
        .assert_exit_code(1)
        .run()
        .await;
}

#[tokio::test]
async fn xargs() {
    TestBuilder::new()
        .command("echo '1   2   3  ' | xargs")
        .assert_stdout("1 2 3\n")
        .run()
        .await;

    TestBuilder::new()
        .command("echo '1   2 \t\t\t3  ' | xargs echo test")
        .assert_stdout("test 1 2 3\n")
        .run()
        .await;

    TestBuilder::new()
        .command(r#"deno eval "console.log('testing\nthis')" | xargs"#)
        .assert_stdout("testing this\n")
        .run()
        .await;

    // \n delimiter
    TestBuilder::new()
    .command(r#"deno eval "console.log('testing this out\n\ntest\n')" | xargs -d \n deno eval "console.log(Deno.args)""#)
    .assert_stdout("[ \"testing this out\", \"\", \"test\", \"\" ]\n")
    .run()
    .await;

    // \0 delimiter
    TestBuilder::new()
    .command(r#"deno eval "console.log('testing this out\ntest\0other')" | xargs -0 deno eval "console.log(Deno.args)""#)
    .assert_stdout("[ \"testing this out\\ntest\", \"other\\n\" ]\n")
    .run()
    .await;

    // unmatched single quote
    TestBuilder::new()
    .command(r#"deno eval "console.log(\"'test\")" | xargs"#)
    .assert_stderr("xargs: unmatched quote; by default quotes are special to xargs unless you use the -0 option\n")
    .assert_exit_code(1)
    .run()
    .await;

    // unmatched double quote
    TestBuilder::new()
    .command(r#"deno eval "console.log('\"test')" | xargs"#)
    .assert_stderr("xargs: unmatched quote; by default quotes are special to xargs unless you use the -0 option\n")
    .assert_exit_code(1)
    .run()
    .await;

    // test reading env file
    TestBuilder::new()
        .file(
            ".env",
            r#"VAR1="testing"
VAR2="other"
"#,
        )
        // most likely people would want to do `export $(grep -v '^#' .env | xargs)` though
        // in order to remove comments...
        .command("export $(cat .env | xargs) && echo $VAR1 $VAR2")
        .assert_stdout("testing other\n")
        .run()
        .await;
}

#[tokio::test]
async fn stdin() {
    TestBuilder::new()
    .command(r#"deno eval "const b = new Uint8Array(1);Deno.stdin.readSync(b);console.log(b)" && deno eval "const b = new Uint8Array(1);Deno.stdin.readSync(b);console.log(b)""#)
    .stdin("12345")
    .assert_stdout("Uint8Array(1) [ 49 ]\nUint8Array(1) [ 50 ]\n")
    .run()
    .await;

    TestBuilder::new()
    .command(r#"echo "12345" | (deno eval "const b = new Uint8Array(1);Deno.stdin.readSync(b);console.log(b)" && deno eval "const b = new Uint8Array(1);Deno.stdin.readSync(b);console.log(b)")"#)
    .stdin("55555") // should not use this because stdin is piped from the echo
    .assert_stdout("Uint8Array(1) [ 49 ]\nUint8Array(1) [ 50 ]\n")
    .run()
    .await;
}

#[cfg(windows)]
#[tokio::test]
async fn windows_resolve_command() {
    // not cross platform, but still allow this
    TestBuilder::new()
        .command("deno.exe eval 'console.log(1)'")
        .assert_stdout("1\n")
        .run()
        .await;

    TestBuilder::new()
        .command("deno eval 'console.log(1)'")
        // handle trailing semi-colon
        .env_var("PATHEXT", ".EXE;")
        .assert_stdout("1\n")
        .run()
        .await;
}

#[tokio::test]
async fn custom_command() {
    // not cross platform, but still allow this
    TestBuilder::new()
        .command("add 1 2")
        .custom_command(
            "add",
            Box::new(|mut context| {
                async move {
                    let mut sum = 0;
                    for val in context.args {
                        sum += val
                            .to_string_lossy()
                            .parse::<usize>()
                            .unwrap();
                    }
                    let _ = context.stderr.write_line(&sum.to_string());
                    ExecuteResult::from_exit_code(0)
                }
                .boxed_local()
            }),
        )
        .assert_stderr("3\n")
        .run()
        .await;
}

#[tokio::test]
async fn custom_command_resolve_command_path() {
    TestBuilder::new()
        .command("$(custom_which deno) eval 'console.log(1)'")
        .custom_command(
            "custom_which",
            Box::new(|mut context| {
                async move {
                    let path = context.state
                        .resolve_command_path(&context.args[0])
                        .unwrap();
                    let _ = context.stdout.write_line(&path.to_string_lossy());
                    ExecuteResult::from_exit_code(0)
                }
                .boxed_local()
            }),
        )
        .assert_stdout("1\n")
        .run()
        .await;
}

#[tokio::test]
async fn glob_basic() {
    TestBuilder::new()
        .file("test.txt", "test\n")
        .file("test2.txt", "test2\n")
        .command("cat *.txt")
        .assert_stdout("test\ntest2\n")
        .run()
        .await;

    TestBuilder::new()
        .file("test.txt", "test\n")
        .file("test2.txt", "test2\n")
        .command("cat test?.txt")
        .assert_stdout("test2\n")
        .run()
        .await;

    TestBuilder::new()
        .file("test.txt", "test\n")
        .file("testa.txt", "testa\n")
        .file("test2.txt", "test2\n")
        .command("cat test[0-9].txt")
        .assert_stdout("test2\n")
        .run()
        .await;

    TestBuilder::new()
        .file("test.txt", "test\n")
        .file("testa.txt", "testa\n")
        .file("test2.txt", "test2\n")
        .command("cat test[!a-z].txt")
        .assert_stdout("test2\n")
        .run()
        .await;

    TestBuilder::new()
        .file("test.txt", "test\n")
        .file("testa.txt", "testa\n")
        .file("test2.txt", "test2\n")
        .command("cat test[a-z].txt")
        .assert_stdout("testa\n")
        .run()
        .await;

    TestBuilder::new()
        .directory("sub_dir/sub")
        .file("sub_dir/sub/1.txt", "1\n")
        .file("sub_dir/2.txt", "2\n")
        .file("sub_dir/other.ts", "other\n")
        .file("3.txt", "3\n")
        .command("cat */*.txt")
        .assert_stdout("2\n")
        .run()
        .await;

    // ** matches recursively (globstar on by default)
    TestBuilder::new()
        .directory("sub_dir/sub")
        .file("sub_dir/sub/1.txt", "1\n")
        .file("sub_dir/2.txt", "2\n")
        .file("sub_dir/other.ts", "other\n")
        .file("3.txt", "3\n")
        .command("cat **/*.txt")
        .assert_stdout("3\n2\n1\n")
        .run()
        .await;

    TestBuilder::new()
        .directory("sub_dir/sub")
        .file("sub_dir/sub/1.txt", "1\n")
        .file("sub_dir/2.txt", "2\n")
        .file("sub_dir/other.ts", "other\n")
        .file("3.txt", "3\n")
        .command("cat $PWD/**/*.txt")
        .assert_stdout("3\n2\n1\n")
        .run()
        .await;

    TestBuilder::new()
        .directory("dir")
        .file("dir/1.txt", "1\n")
        .file("dir_1.txt", "2\n")
        .command("cat dir*1.txt")
        .assert_stdout("2\n")
        .run()
        .await;

    TestBuilder::new()
    .file("test.txt", "test\n")
    .file("test2.txt", "test2\n")
    .command("shopt -s failglob && cat *.ts")
    .assert_stderr("glob: no matches found '$TEMP_DIR/*.ts' (run `shopt -u failglob` to pass unmatched glob patterns literally)\n")
    .assert_exit_code(1)
    .run()
    .await;

    let mut builder = TestBuilder::new();
    let temp_dir_path = builder.temp_dir_path();
    let error_pos = temp_dir_path.to_string_lossy().len() + 1;
    builder.file("test.txt", "test\n")
    .file("test2.txt", "test2\n")
    .command("shopt -s failglob && cat [].ts")
    .assert_stderr(&format!("glob: no matches found '$TEMP_DIR/[].ts'. Pattern syntax error near position {}: invalid range pattern\n", error_pos))
    .assert_exit_code(1)
    .run()
    .await;

    TestBuilder::new()
    .file("test.txt", "test\n")
    .file("test2.txt", "test2\n")
    .command("shopt -s failglob && cat *.ts || echo 2")
    .assert_stderr("glob: no matches found '$TEMP_DIR/*.ts' (run `shopt -u failglob` to pass unmatched glob patterns literally)\n")
    .assert_stdout("2\n")
    .assert_exit_code(0)
    .run()
    .await;

    TestBuilder::new()
        .file("test.txt", "test\n")
        .file("test2.txt", "test2\n")
        .command("cat *.ts 2> /dev/null || echo 2")
        .assert_stderr("")
        .assert_stdout("2\n")
        .assert_exit_code(0)
        .run()
        .await;

    TestBuilder::new()
        .command("echo --inspect='[::0]:3366'")
        .assert_stderr("")
        .assert_stdout("--inspect=[::0]:3366\n")
        .assert_exit_code(0)
        .run()
        .await;

    // denoland/deno#27238: a glob inside a non-path argument like a flag value
    // should pass through literally rather than erroring.
    TestBuilder::new()
        .command("echo --allow-write=@mizu/**/README.md,README.md")
        .assert_stderr("")
        .assert_stdout("--allow-write=@mizu/**/README.md,README.md\n")
        .assert_exit_code(0)
        .run()
        .await;

    // Same issue, malformed-pattern form: `**` not in its own path component.
    // The glob crate rejects the pattern; without failglob we should still
    // fall through to the literal text.
    TestBuilder::new()
        .command("echo --allow-write=**/*.txt")
        .assert_stderr("")
        .assert_stdout("--allow-write=**/*.txt\n")
        .assert_exit_code(0)
        .run()
        .await;
}

#[tokio::test]
async fn glob_case_insensitive() {
    TestBuilder::new()
        .file("TEST.txt", "test\n")
        .file("testa.txt", "testa\n")
        .file("test2.txt", "test2\n")
        .command("cat tes*.txt")
        .assert_stdout("test\ntest2\ntesta\n")
        .run()
        .await;
}

#[tokio::test]
async fn glob_escapes() {
    // no escape
    TestBuilder::new()
        .file("[test].txt", "test\n")
        .file("t.txt", "t\n")
        .command("cat [test].txt")
        .assert_stdout("t\n")
        .run()
        .await;

    // escape
    TestBuilder::new()
        .file("[test].txt", "test\n")
        .file("t.txt", "t\n")
        .command("cat [[]test[]].txt")
        .assert_stdout("test\n")
        .run()
        .await;

    // single quotes
    TestBuilder::new()
        .file("[test].txt", "test\n")
        .file("t.txt", "t\n")
        .command("cat '[test].txt'")
        .assert_stdout("test\n")
        .run()
        .await;

    // double quotes
    TestBuilder::new()
        .file("[test].txt", "test\n")
        .file("t.txt", "t\n")
        .command("cat \"[test].txt\"")
        .assert_stdout("test\n")
        .run()
        .await;

    // mix
    TestBuilder::new()
        .file("[test].txt", "test\n")
        .file("t.txt", "t\n")
        .command("cat \"[\"test\"]\".txt")
        .assert_stdout("test\n")
        .run()
        .await;
}

#[tokio::test]
async fn paren_escapes() {
    TestBuilder::new()
        .command(r"echo \( foo bar \)")
        .assert_stdout("( foo bar )\n")
        .run()
        .await;
}

#[tokio::test]
async fn tilde_expansion() {
    let text = concat!(
        r"export HOME=/home/dir && echo ~/test && echo ~ && ",
        r"echo \~ && export HOME=/test && echo ~ && ",
        r"HOME=/nope echo ~ && ",
        r"HOME=/nope $(echo echo ~) && ",
        r"echo ~/sub/~/sub"
    );
    let text = if cfg!(windows) {
        // windows uses a different env var
        text.replace("HOME", "USERPROFILE")
    } else {
        text.to_string()
    };
    TestBuilder::new()
        .command(&text)
        .assert_stdout("/home/dir/test\n/home/dir\n~\n/test\n/test\n/test\n/test/sub/~/sub\n")
        .run()
        .await;
}

#[tokio::test]
async fn cross_platform_shebang() {
    // with -S
    TestBuilder::new()
        .file("file.ts", "#!/usr/bin/env -S deno run\nconsole.log(5)")
        .command("./file.ts")
        .assert_stdout("5\n")
        .run()
        .await;

    // without -S and invalid
    TestBuilder::new()
        .file("file.ts", "#!/usr/bin/env deno run\nconsole.log(5)")
        .command("./file.ts")
        .assert_stderr("deno run: command not found\n")
        .assert_exit_code(127)
        .run()
        .await;

    // without -S, but valid
    TestBuilder::new()
    .file("file.ts", "#!/usr/bin/env ./echo_stdin.ts\nconsole.log('Hello')")
    .file("echo_stdin.ts", "#!/usr/bin/env -S deno run --allow-all\nawait new Deno.Command('deno', { args: ['run', ...Deno.args] }).spawn();")
    .command("./file.ts")
    .assert_stdout("Hello\n")
    .run()
    .await;

    // sub dir
    TestBuilder::new()
    .directory("sub")
    .file("sub/file.ts", "#!/usr/bin/env ../echo_stdin.ts\nconsole.log('Hello')")
    .file("echo_stdin.ts", "#!/usr/bin/env -S deno run --allow-all\nawait new Deno.Command('deno', { args: ['run', ...Deno.args] }).spawn();")
    .command("./sub/file.ts")
    .assert_stdout("Hello\n")
    .run()
    .await;

    // arguments
    TestBuilder::new()
    .file(
      "file.ts",
      "#!/usr/bin/env -S deno run --allow-read\nconsole.log(Deno.args)\nconst text = Deno.readTextFileSync(import.meta.filename);\nconsole.log(text.length)\n",
    )
    .command("./file.ts 1 2 3")
    .assert_stdout(r#"[ "1", "2", "3" ]
146
"#)
    .run()
    .await;
}

#[tokio::test]
async fn provided_signal_cancel() {
    let start_time = Instant::now();
    {
        let kill_signal = KillSignal::default();
        kill_signal.send(SignalKind::SIGINT);
        TestBuilder::new()
            .command("sleep 5")
            .kill_signal(kill_signal)
            .assert_exit_code(130)
            .run()
            .await;
    }
    {
        let kill_signal = KillSignal::default();
        kill_signal.send(SignalKind::SIGKILL);
        TestBuilder::new()
            .command("sleep 5")
            .kill_signal(kill_signal)
            .assert_exit_code(137)
            .run()
            .await;
    }
    assert!(start_time.elapsed().as_secs() < 1);
}

#[tokio::test]
async fn listens_for_signals_exits_gracefully() {
    if cfg!(windows) {
        return; // signals are terrible on windows
    }

    let kill_signal = KillSignal::default();
    deno_unsync::spawn({
        let kill_signal = kill_signal.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            kill_signal.send(SignalKind::SIGINT);
        }
    });
    TestBuilder::new()
      .command(r#"deno eval 'Deno.addSignalListener("SIGINT", () => { console.log("interrupted!"); setTimeout(() => Deno.exit(0), 25); }); setInterval(() => {}, 1000)'"#)
      .kill_signal(kill_signal)
      .assert_exit_code(0) // signal was handled and a `Deno.exit(0)` was done
      .assert_stdout("interrupted!\n")
      .run()
      .await;
}

fn no_such_file_error_text() -> &'static str {
    if cfg!(windows) {
        "The system cannot find the file specified. (os error 2)"
    } else {
        "No such file or directory (os error 2)"
    }
}

#[tokio::test]
#[cfg(unix)]
async fn signal_exit_codes() {
    // Test SIGPIPE exit code (128 + 13 = 141)
    TestBuilder::new()
        .command("sh -c 'kill -PIPE $$'")
        .assert_exit_code(141)
        .run()
        .await;

    // Test SIGTERM exit code (128 + 15 = 143)
    TestBuilder::new()
        .command("sh -c 'kill -TERM $$'")
        .assert_exit_code(143)
        .run()
        .await;

    // Test SIGINT exit code (128 + 2 = 130)
    TestBuilder::new()
        .command("sh -c 'kill -INT $$'")
        .assert_exit_code(130)
        .run()
        .await;

    // Test SIGKILL exit code (128 + 9 = 137)
    TestBuilder::new()
        .command("sh -c 'kill -KILL $$'")
        .assert_exit_code(137)
        .run()
        .await;
}

#[tokio::test]
#[cfg(unix)]
async fn sigpipe_from_pipeline() {
    // Test that SIGPIPE from a closed pipe doesn't cause issues
    // and the pipeline completes successfully
    // `yes` outputs infinite "y\n" until SIGPIPE, `head -n 1` reads one line
    TestBuilder::new()
        .command("yes | head -n 1")
        .assert_stdout("y\n")
        .assert_exit_code(0)
        .run()
        .await;
}

#[tokio::test]
async fn shopt() {
    // query all options (default: failglob off, globstar on, nullglob off)
    TestBuilder::new()
        .command("shopt")
        .assert_stdout("failglob\toff\nglobstar\ton\nnullglob\toff\n")
        .run()
        .await;

    // query specific option
    TestBuilder::new()
        .command("shopt nullglob")
        .assert_stdout("nullglob\toff\n")
        .assert_exit_code(1) // returns 1 when option is off
        .run()
        .await;

    TestBuilder::new()
        .command("shopt failglob")
        .assert_stdout("failglob\toff\n")
        .assert_exit_code(1) // returns 1 when option is off
        .run()
        .await;

    // enable option
    TestBuilder::new()
        .command("shopt -s nullglob && shopt nullglob")
        .assert_stdout("nullglob\ton\n")
        .assert_exit_code(0)
        .run()
        .await;

    // enable failglob
    TestBuilder::new()
        .command("shopt -s failglob && shopt failglob")
        .assert_stdout("failglob\ton\n")
        .assert_exit_code(0)
        .run()
        .await;

    // invalid option name
    TestBuilder::new()
        .command("shopt -s invalidopt")
        .assert_stderr("shopt: invalidopt: invalid shell option name\n")
        .assert_exit_code(1)
        .run()
        .await;

    // -s without option name
    TestBuilder::new()
        .command("shopt -s")
        .assert_stderr("shopt: option name required\n")
        .assert_exit_code(1)
        .run()
        .await;

    // cannot set and unset simultaneously
    TestBuilder::new()
        .command("shopt -s -u nullglob")
        .assert_stderr("shopt: cannot set and unset options simultaneously\n")
        .assert_exit_code(1)
        .run()
        .await;

    // multiple options
    TestBuilder::new()
        .command("shopt -s nullglob && shopt -s failglob && shopt")
        .assert_stdout("failglob\ton\nglobstar\ton\nnullglob\ton\n")
        .run()
        .await;

    // query globstar option (on by default)
    TestBuilder::new()
        .command("shopt globstar")
        .assert_stdout("globstar\ton\n")
        .assert_exit_code(0) // returns 0 when option is on
        .run()
        .await;

    // disable globstar
    TestBuilder::new()
        .command("shopt -u globstar && shopt globstar")
        .assert_stdout("globstar\toff\n")
        .assert_exit_code(1)
        .run()
        .await;
}

#[tokio::test]
async fn shopt_nullglob() {
    // default behavior (failglob off): unmatched glob passed literally
    TestBuilder::new()
        .file("test.txt", "test\n")
        .command("echo *.nonexistent")
        .assert_stdout("*.nonexistent\n")
        .assert_exit_code(0)
        .run()
        .await;

    // with nullglob: unmatched glob expands to nothing
    TestBuilder::new()
        .file("test.txt", "test\n")
        .command("shopt -s nullglob && echo *.nonexistent")
        .assert_stdout("\n") // echo with no args outputs newline
        .assert_exit_code(0)
        .run()
        .await;

    // nullglob with other args: unmatched glob removed, other args kept
    TestBuilder::new()
        .file("test.txt", "test\n")
        .command("shopt -s nullglob && echo hello *.nonexistent world")
        .assert_stdout("hello world\n")
        .assert_exit_code(0)
        .run()
        .await;

    // nullglob: matched glob still works normally
    TestBuilder::new()
        .file("test.txt", "test\n")
        .command("shopt -s nullglob && cat *.txt")
        .assert_stdout("test\n")
        .assert_exit_code(0)
        .run()
        .await;
}

#[tokio::test]
async fn shopt_failglob() {
    // failglob off (default): unmatched glob passed through literally
    TestBuilder::new()
        .file("test.txt", "test\n")
        .command("echo *.nonexistent")
        .assert_stdout("*.nonexistent\n")
        .assert_exit_code(0)
        .run()
        .await;

    // failglob on: unmatched glob causes error
    TestBuilder::new()
    .file("test.txt", "test\n")
    .command("shopt -s failglob && echo *.nonexistent")
    .assert_stderr("glob: no matches found '$TEMP_DIR/*.nonexistent' (run `shopt -u failglob` to pass unmatched glob patterns literally)\n")
    .assert_exit_code(1)
    .run()
    .await;

    // failglob takes precedence over nullglob
    TestBuilder::new()
    .file("test.txt", "test\n")
    .command("shopt -s nullglob && shopt -s failglob && echo *.nonexistent")
    .assert_stderr("glob: no matches found '$TEMP_DIR/*.nonexistent' (run `shopt -u failglob` to pass unmatched glob patterns literally)\n")
    .assert_exit_code(1)
    .run()
    .await;

    // failglob off, nullglob off: literal pattern (bash default)
    TestBuilder::new()
        .file("test.txt", "test\n")
        .command("shopt -u failglob && shopt -u nullglob && echo *.nonexistent")
        .assert_stdout("*.nonexistent\n")
        .assert_exit_code(0)
        .run()
        .await;

    // failglob on with a malformed pattern still errors (exit code 1)
    TestBuilder::new()
        .command("shopt -s failglob && echo --allow-write=**/*.txt 2> /dev/null || echo recovered")
        .assert_stderr("")
        .assert_stdout("recovered\n")
        .assert_exit_code(0)
        .run()
        .await;
}

#[tokio::test]
async fn shopt_globstar() {
    // globstar on by default: ** matches zero or more directories
    // use cat to verify all files are matched (content order verifies glob worked)
    TestBuilder::new()
        .directory("sub/deep")
        .file("a.txt", "a\n")
        .file("sub/b.txt", "b\n")
        .file("sub/deep/c.txt", "c\n")
        .command("cat **/*.txt")
        .assert_stdout("a\nb\nc\n")
        .assert_exit_code(0)
        .run()
        .await;

    // single * still behaves normally even with globstar enabled
    TestBuilder::new()
        .directory("sub")
        .file("a.txt", "a\n")
        .file("sub/b.txt", "b\n")
        .command("echo *.txt")
        .assert_stdout("a.txt\n")
        .assert_exit_code(0)
        .run()
        .await;

    // disabling globstar: ** behaves like * (single-segment match)
    TestBuilder::new()
        .directory("sub/deep")
        .file("a.txt", "a\n")
        .file("sub/b.txt", "b\n")
        .file("sub/deep/c.txt", "c\n")
        .command("shopt -u globstar && cat **/*.txt")
        .assert_stdout("b\n") // only matches one level deep (sub/b.txt)
        .assert_exit_code(0)
        .run()
        .await;

    // re-enabling globstar restores recursive behavior
    TestBuilder::new()
        .directory("sub/deep")
        .file("a.txt", "a\n")
        .file("sub/b.txt", "b\n")
        .file("sub/deep/c.txt", "c\n")
        .command("shopt -u globstar && shopt -s globstar && cat **/*.txt")
        .assert_stdout("a\nb\nc\n")
        .assert_exit_code(0)
        .run()
        .await;
}

#[tokio::test]
async fn pipefail_option() {
    // Without pipefail: exit code is from last command (0)
    TestBuilder::new()
        .command("(exit 1) | true")
        .assert_exit_code(0)
        .run()
        .await;

    // With pipefail: exit code is rightmost non-zero (1)
    TestBuilder::new()
        .command("set -o pipefail && (exit 1) | true")
        .assert_exit_code(1)
        .run()
        .await;

    // Multiple failures - should return rightmost non-zero
    TestBuilder::new()
        .command("set -o pipefail && (exit 2) | (exit 3) | true")
        .assert_exit_code(3)
        .run()
        .await;

    // All succeed - should return 0
    TestBuilder::new()
        .command("set -o pipefail && true | true | true")
        .assert_exit_code(0)
        .run()
        .await;

    // Disable pipefail with +o
    TestBuilder::new()
        .command("set -o pipefail && set +o pipefail && (exit 1) | true")
        .assert_exit_code(0)
        .run()
        .await;

    // invalid option name
    TestBuilder::new()
        .command("set -o invalidopt")
        .assert_stderr("set: unknown option: invalidopt\n")
        .assert_exit_code(1)
        .run()
        .await;

    // invalid option
    TestBuilder::new()
        .command("set -x")
        .assert_stderr("set: invalid option: -x\n")
        .assert_exit_code(1)
        .run()
        .await;

    // invalid option (non-option argument)
    TestBuilder::new()
        .command("set foo")
        .assert_stderr("set: invalid option: foo\n")
        .assert_exit_code(1)
        .run()
        .await;

    // set -o (list options, readable format)
    TestBuilder::new()
        .command("set -o")
        .assert_stdout("errexit\toff\npipefail\toff\n")
        .assert_exit_code(0)
        .run()
        .await;

    // set -o after enabling pipefail
    TestBuilder::new()
        .command("set -o pipefail && set -o")
        .assert_stdout("errexit\toff\npipefail\ton\n")
        .assert_exit_code(0)
        .run()
        .await;

    // set +o (list options, reusable format)
    TestBuilder::new()
        .command("set +o")
        .assert_stdout("set +o errexit\nset +o pipefail\n")
        .assert_exit_code(0)
        .run()
        .await;

    // set +o after enabling pipefail
    TestBuilder::new()
        .command("set -o pipefail && set +o")
        .assert_stdout("set +o errexit\nset -o pipefail\n")
        .assert_exit_code(0)
        .run()
        .await;
}

#[tokio::test]
#[cfg(unix)]
async fn pipefail_with_sigpipe() {
    // With pipefail and SIGPIPE: should return 141 (128 + 13)
    TestBuilder::new()
        .command("set -o pipefail && yes | head -n 1")
        .assert_stdout("y\n")
        .assert_exit_code(141)
        .run()
        .await;
}

#[tokio::test]
async fn multi_line_commands() {
    // newline separates commands, like `;`
    TestBuilder::new()
        .command("echo one\necho two\necho three")
        .assert_stdout("one\ntwo\nthree\n")
        .run()
        .await;

    // blank lines are ignored
    TestBuilder::new()
        .command("\n\necho foo\n\necho bar\n")
        .assert_stdout("foo\nbar\n")
        .run()
        .await;

    // newline after `&&` is a continuation
    TestBuilder::new()
        .command("echo foo &&\n  echo bar")
        .assert_stdout("foo\nbar\n")
        .run()
        .await;

    // newline after `|` is a continuation
    TestBuilder::new()
        .command("echo hello |\n  cat")
        .assert_stdout("hello\n")
        .run()
        .await;

    // env assignment on its own line sets a shell var for later lines
    TestBuilder::new()
        .command("FOO=bar\necho $FOO")
        .assert_stdout("bar\n")
        .run()
        .await;

    // CRLF line endings work
    TestBuilder::new()
        .command("echo one\r\necho two")
        .assert_stdout("one\ntwo\n")
        .run()
        .await;
}

#[tokio::test]
async fn errexit_option() {
    // default: failures between `;`-separated commands don't abort
    TestBuilder::new()
        .command("(exit 3); echo kept-going")
        .assert_stdout("kept-going\n")
        .assert_exit_code(0)
        .run()
        .await;

    // with `set -e`: abort at the first non-zero command
    TestBuilder::new()
        .command("set -e; (exit 3); echo unreachable")
        .assert_exit_code(3)
        .run()
        .await;

    // `set -e` then `set +e` should re-enable continue-on-failure
    TestBuilder::new()
        .command("set -e; set +e; (exit 3); echo kept-going")
        .assert_stdout("kept-going\n")
        .assert_exit_code(0)
        .run()
        .await;

    // `set -o errexit` long form is equivalent to `set -e`
    TestBuilder::new()
        .command("set -o errexit; (exit 3); echo unreachable")
        .assert_exit_code(3)
        .run()
        .await;

    // with errexit on, newline-separated also aborts
    TestBuilder::new()
        .command("set -e\n(exit 3)\necho unreachable")
        .assert_exit_code(3)
        .run()
        .await;

    // errexit does NOT abort commands whose failure feeds a `||`
    TestBuilder::new()
        .command("set -e; (exit 3) || echo recovered; echo after")
        .assert_stdout("recovered\nafter\n")
        .assert_exit_code(0)
        .run()
        .await;
}

#[tokio::test]
async fn test_builtin() {
    // single-arg string truthiness
    TestBuilder::new()
        .command("test hello && echo ok")
        .assert_stdout("ok\n")
        .run()
        .await;
    TestBuilder::new()
        .command("test '' || echo empty")
        .assert_stdout("empty\n")
        .run()
        .await;

    // string predicates
    TestBuilder::new()
        .command("test -n hello && echo yes")
        .assert_stdout("yes\n")
        .run()
        .await;
    TestBuilder::new()
        .command("test -z '' && echo yes")
        .assert_stdout("yes\n")
        .run()
        .await;

    // string comparison
    TestBuilder::new()
        .command("test a = a && test a != b && echo ok")
        .assert_stdout("ok\n")
        .run()
        .await;

    // integer comparison
    TestBuilder::new()
        .command("test 1 -lt 2 && test 3 -ge 3 && echo ok")
        .assert_stdout("ok\n")
        .run()
        .await;

    // negation
    TestBuilder::new()
        .command("test ! '' && echo ok")
        .assert_stdout("ok\n")
        .run()
        .await;
    TestBuilder::new()
        .command("test ! 1 -eq 2 && echo ok")
        .assert_stdout("ok\n")
        .run()
        .await;

    // file predicates: -f, -d, -e, -s
    TestBuilder::new()
        .file("a.txt", "hi")
        .file("empty.txt", "")
        .directory("sub")
        .command("test -f a.txt && test -d sub && test -e a.txt && echo ok")
        .assert_stdout("ok\n")
        .run()
        .await;
    TestBuilder::new()
        .file("a.txt", "hi")
        .file("empty.txt", "")
        .command("test -s a.txt && ! test -s empty.txt && ! test -e missing && echo ok")
        .assert_stdout("ok\n")
        .run()
        .await;

    // syntax error → exit code 2
    TestBuilder::new()
        .command("test foo -unknown bar")
        .assert_stderr("test: binary operator expected: -unknown\n")
        .assert_exit_code(2)
        .run()
        .await;

    // common use case: gating a command on a file existing
    TestBuilder::new()
        .file("artifact", "x")
        .command("test -f artifact && echo found || echo missing")
        .assert_stdout("found\n")
        .run()
        .await;
    TestBuilder::new()
        .command("test -f artifact && echo found || echo missing")
        .assert_stdout("missing\n")
        .run()
        .await;
}
