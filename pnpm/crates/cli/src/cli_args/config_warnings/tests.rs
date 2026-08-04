use super::take_unemitted;
use pacquet_config::Config;
use pretty_assertions::assert_eq;

/// One command can load `Config` more than once — `pnpm install`'s fast path
/// falls through to `run`, and `patch-commit` calls its `state` closure twice —
/// and each load re-collects the same warnings off the same files. pnpm prints
/// each one once per command, so the repeats must not reach stderr.
#[test]
fn a_repeated_config_load_reports_each_warning_once() {
    let mut first = Config { config_warnings: vec![warning("a"), warning("b")], ..Config::new() };
    assert_eq!(take_unemitted(&mut first), vec![warning("a"), warning("b")]);
    assert!(first.config_warnings.is_empty(), "the drained config keeps no warnings");

    let mut reload =
        Config { config_warnings: vec![warning("a"), warning("b"), warning("c")], ..Config::new() };
    assert_eq!(take_unemitted(&mut reload), vec![warning("c")]);
}

/// Namespaced so a warning raised by another test in this process — the static
/// sink is process-wide — can never collide with these.
fn warning(id: &str) -> String {
    format!("config_warnings::tests warning {id}")
}
