use super::{load, scan, strip_flags};
use std::{ffi::OsString, path::PathBuf, sync::Mutex};

/// Build an argv (with a leading program name) from string slices.
fn argv(tokens: &[&str]) -> Vec<OsString> {
    std::iter::once("pnpm")
        .chain(tokens.iter().copied())
        .map(OsString::from)
        .collect()
}

fn strings(argv: &[OsString]) -> Vec<String> {
    argv.iter()
        .map(|token| token.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn strips_the_separate_value_form() {
    let (paths, remaining) = scan(&argv(&["--env-file", ".env", "install"]));
    assert_eq!(paths, vec![PathBuf::from(".env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install"]);
}

#[test]
fn strips_the_inline_value_form() {
    let (paths, remaining) = scan(&argv(&["--env-file=.env", "install"]));
    assert_eq!(paths, vec![PathBuf::from(".env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install"]);
}

#[test]
fn collects_repeated_flags_in_order() {
    let (paths, remaining) = scan(&argv(&["--env-file", "a.env", "--env-file=b.env", "install"]));
    assert_eq!(paths, vec![PathBuf::from("a.env"), PathBuf::from("b.env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install"]);
}

#[test]
fn accepts_the_flag_after_the_subcommand() {
    let (paths, remaining) = scan(&argv(&["install", "--env-file", ".env"]));
    assert_eq!(paths, vec![PathBuf::from(".env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install"]);
}

#[test]
fn leaves_script_arguments_alone() {
    for tokens in [
        vec!["run", "build", "--env-file", ".env"],
        vec!["exec", "cmd", "--env-file=.env"],
        vec!["install", "--", "--env-file", ".env"],
    ] {
        let argv = argv(&tokens);
        let (paths, remaining) = scan(&argv);
        assert!(paths.is_empty(), "claims a script argument: {tokens:?}");
        assert_eq!(remaining, argv, "rewrites a script argument: {tokens:?}");
    }
}

#[test]
fn leaves_a_valueless_flag_for_clap_to_report() {
    let valueless = argv(&["--env-file"]);
    let (paths, remaining) = scan(&valueless);
    assert!(paths.is_empty());
    assert_eq!(remaining, valueless);

    let followed_by_option = argv(&["--env-file", "--verbose", "install"]);
    let (paths, remaining) = scan(&followed_by_option);
    assert!(paths.is_empty(), "claims an option as the path");
    assert_eq!(remaining, followed_by_option);
}

#[test]
fn does_not_claim_the_flag_as_another_options_value() {
    // `--dir` consumes `--env-file` as its value, so the scan must not
    // treat it as the flag.
    let argv = argv(&["--dir", "--env-file", "install"]);
    let (paths, remaining) = scan(&argv);
    assert!(paths.is_empty());
    assert_eq!(remaining, argv);
}

#[test]
fn strips_the_flag_after_the_pm_prefix() {
    // `pm` forces the built-in command but is not a subcommand, so the
    // boundary must be computed as if it were absent.
    let (paths, remaining) = scan(&argv(&["pm", "install", "--env-file", ".env"]));
    assert_eq!(paths, vec![PathBuf::from(".env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "pm", "install"]);
}

#[test]
fn claims_the_flag_when_pm_is_not_the_prefix() {
    // `pm` past the first token is an ordinary argument (here: a package
    // name), so `--env-file` is still pnpm's own flag.
    let (paths, remaining) = scan(&argv(&["install", "pm", "--env-file", ".env"]));
    assert_eq!(paths, vec![PathBuf::from(".env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install", "pm"]);
}

#[cfg(unix)]
#[test]
fn claims_a_non_utf8_value() {
    use std::os::unix::ffi::OsStringExt;
    let raw = vec![0x66, 0x6F, 0xFF];
    let mut owned = argv(&["--env-file", "placeholder", "install"]);
    owned[2] = OsString::from_vec(raw.clone());
    let (paths, remaining) = scan(&owned);
    assert_eq!(paths, vec![PathBuf::from(OsString::from_vec(raw))]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install"]);
}

#[test]
fn strip_flags_keeps_everything_else() {
    let stripped =
        strip_flags(&argv(&["--recursive", "--env-file", "a.env", "install", "--env-file=b.env"]));
    assert_eq!(strings(&stripped), vec!["pnpm", "--recursive", "install"]);
}

/// The environment is process-global and unit tests run on threads sharing
/// it, so every test that mutates it holds this lock and uses a key no
/// other test touches.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn unique_key(name: &str) -> String {
    format!("PACQUET_ENV_FILE_TEST_{name}")
}

fn write_env_file(dir: &std::path::Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write env file fixture");
    path
}

#[test]
fn load_layers_files_first_wins() {
    let guard = ENV_LOCK.lock().expect("take the env lock");
    let key = unique_key("FIRST_WINS");
    // SAFETY: the env lock is held, so no other test thread observes this.
    unsafe {
        std::env::remove_var(&key);
    }
    let dir = tempfile::tempdir().expect("create fixture dir");
    let first = write_env_file(dir.path(), "first.env", format!("{key}=first\n").as_str());
    let second = write_env_file(dir.path(), "second.env", format!("{key}=second\n").as_str());

    load(&[first, second]).expect("load env files");

    assert_eq!(std::env::var(&key).expect("read layered var"), "first");
    // SAFETY: the env lock is held, so no other test thread observes this.
    unsafe {
        std::env::remove_var(&key);
    }
    drop(guard);
}

#[test]
fn load_never_overrides_the_environment() {
    let guard = ENV_LOCK.lock().expect("take the env lock");
    let key = unique_key("NO_OVERRIDE");
    let dir = tempfile::tempdir().expect("create fixture dir");
    let path = write_env_file(dir.path(), "override.env", format!("{key}=from-file\n").as_str());

    // SAFETY: the env lock is held, so no other test thread observes this.
    unsafe {
        std::env::set_var(&key, "from-env");
    }
    load(std::slice::from_ref(&path)).expect("load env file");

    assert_eq!(std::env::var(&key).expect("read guarded var"), "from-env");
    // SAFETY: the env lock is held, so no other test thread observes this.
    unsafe {
        std::env::remove_var(&key);
    }
    drop(guard);
}

#[test]
fn load_fails_loudly_on_a_missing_file() {
    let _guard = ENV_LOCK.lock().expect("take the env lock");
    let missing = PathBuf::from("pacquet-env-file-test-does-not-exist.env");
    let error = load(std::slice::from_ref(&missing)).expect_err("missing file must fail");
    let message = format!("{error:?}");
    assert!(
        message.contains("pacquet-env-file-test-does-not-exist.env"),
        "error names the file: {message}",
    );
}

#[test]
fn load_fails_loudly_on_a_malformed_file() {
    let _guard = ENV_LOCK.lock().expect("take the env lock");
    let dir = tempfile::tempdir().expect("create fixture dir");
    let path = write_env_file(dir.path(), "malformed.env", "THIS LINE HAS NO EQUALS\n");
    let error = load(std::slice::from_ref(&path)).expect_err("malformed file must fail");
    let message = format!("{error:?}");
    assert!(message.contains("malformed.env"), "error names the file: {message}");
}

#[test]
fn load_redacts_file_contents_from_parse_errors() {
    let _guard = ENV_LOCK.lock().expect("take the env lock");
    let dir = tempfile::tempdir().expect("create fixture dir");
    let path = write_env_file(
        dir.path(),
        "secret.env",
        "PACQUET_ENV_FILE_TEST_SECRET_LEAK no equals here\n",
    );
    let error = load(std::slice::from_ref(&path)).expect_err("malformed file must fail");
    let message = format!("{error:?}");
    assert!(message.contains("secret.env"), "error names the file: {message}");
    assert!(
        !message.contains("PACQUET_ENV_FILE_TEST_SECRET_LEAK"),
        "error must not echo file contents: {message}",
    );
}

#[test]
fn load_rejects_nul_bytes_instead_of_panicking() {
    let guard = ENV_LOCK.lock().expect("take the env lock");
    let key = unique_key("NUL");
    let dir = tempfile::tempdir().expect("create fixture dir");
    let path = dir.path().join("nul.env");
    std::fs::write(&path, format!("{key}=before\0after\n")).expect("write env file fixture");
    let error = load(std::slice::from_ref(&path)).expect_err("NUL value must fail");
    let message = format!("{error:?}");
    assert!(message.contains("nul.env"), "error names the file: {message}");
    assert!(message.contains("NUL"), "error names the cause: {message}");
    assert!(std::env::var_os(&key).is_none(), "rejected value must not be set");
    drop(guard);
}
