use super::{
    cmd_escape,
    sh_single_quote,
};
use pnpm_process::Command;
use pretty_assertions::assert_eq;

#[test]
fn quotes_for_sh() {
    assert_eq!(sh_single_quote("bin"), "'bin'");
    assert_eq!(sh_single_quote(""), "''");
    assert_eq!(sh_single_quote("it's"), r"'it'\''s'");
    assert_eq!(sh_single_quote("'"), r"''\'''");
}

#[test]
fn doubles_percent_for_cmd() {
    assert_eq!(cmd_escape("100%"), "100%%");
    assert_eq!(cmd_escape("%PATH%"), "%%PATH%%");
    assert_eq!(cmd_escape("plain"), "plain");
}

/// `sh` reads back what `sh_single_quote` wrote, for text holding
/// characters the shell treats specially, such as quotes, `$`, backquotes and
/// operators, so the reading the proof uses agrees with a real shell on them.
#[cfg_attr(windows, ignore = "needs a POSIX sh")]
#[test]
fn sh_reads_back_the_quoted_text() {
    let texts =
        ["plain", "it's", "''", r"\'", "a b", "$HOME `id` $(id)", "a;b|c&d>e<f", "\"*?[]~#!\n\t"];
    for text in texts {
        let output = Command::new("sh")
            .arg("-c")
            .arg(format!("printf %s {}", sh_single_quote(text)))
            .output()
            .expect("run sh");
        assert_eq!(String::from_utf8(output.stdout).expect("UTF-8 output"), text, "{text:?}");
    }
}
