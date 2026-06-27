//! Ports the rendering and config-file-editing cases of pnpm's
//! [`path-extender-posix.spec.ts`](https://github.com/pnpm/pnpm/blob/1819226b51/packages/path-extender-posix/test/index.ts).
//! The shell-detection / home-resolution wrapper reads process-global env
//! state, so the tests exercise the pure renderers and the file editor
//! directly (with a `tempfile` config file) instead of through
//! `add_dir_to_posix_env_path`.

use super::{
    AddDirToEnvPathOpts, AddingPosition, ConfigFileChangeType, PathExtenderError, find_section,
    render_fish_settings, render_nu_settings, render_posix_settings, replace_section,
    update_shell_config, wrap_settings,
};
use pretty_assertions::assert_eq;
use std::fs;

const HOME: &str = "/home/user/.pnpm";

fn opts(overwrite: bool) -> AddDirToEnvPathOpts<'static> {
    AddDirToEnvPathOpts {
        config_section_name: "pnpm",
        proxy_var_name: Some("PNPM_HOME"),
        proxy_var_sub_dir: None,
        overwrite,
        position: AddingPosition::Start,
    }
}

#[test]
fn bash_settings_with_proxy_variable() {
    let settings = render_posix_settings(HOME, &opts(false));
    assert_eq!(
        settings,
        r#"export PNPM_HOME='/home/user/.pnpm'
case ":$PATH:" in
  *":$PNPM_HOME:"*) ;;
  *) export PATH="$PNPM_HOME:$PATH" ;;
esac"#,
    );
}

#[test]
fn bash_settings_without_proxy_variable() {
    let mut opts = opts(false);
    opts.proxy_var_name = None;
    let settings = render_posix_settings(HOME, &opts);
    assert_eq!(
        settings,
        r#"case ":$PATH:" in
  *":"'/home/user/.pnpm'":"*) ;;
  *) export PATH='/home/user/.pnpm':$PATH ;;
esac"#,
    );
}

#[test]
fn bash_settings_with_proxy_var_sub_dir() {
    let mut opts = opts(false);
    opts.proxy_var_sub_dir = Some("bin");
    let settings = render_posix_settings(HOME, &opts);
    assert_eq!(
        settings,
        r#"export PNPM_HOME='/home/user/.pnpm'
case ":$PATH:" in
  *":$PNPM_HOME/bin:"*) ;;
  *) export PATH="$PNPM_HOME/bin:$PATH" ;;
esac"#,
    );
}

#[test]
fn bash_settings_appending_to_the_end_of_path() {
    let mut opts = opts(false);
    opts.position = AddingPosition::End;
    let settings = render_posix_settings(HOME, &opts);
    assert_eq!(
        settings,
        r#"export PNPM_HOME='/home/user/.pnpm'
case ":$PATH:" in
  *":$PNPM_HOME:"*) ;;
  *) export PATH="$PATH:$PNPM_HOME" ;;
esac"#,
    );
}

#[test]
fn fish_settings_with_proxy_var_sub_dir() {
    let mut opts = opts(false);
    opts.proxy_var_sub_dir = Some("bin");
    let settings = render_fish_settings(HOME, &opts);
    assert_eq!(
        settings,
        r#"set -gx PNPM_HOME '/home/user/.pnpm'
if not string match -q -- "$PNPM_HOME/bin" $PATH
  set -gx PATH "$PNPM_HOME/bin" $PATH
end"#,
    );
}

#[test]
fn fish_settings_without_proxy_variable() {
    let mut opts = opts(false);
    opts.proxy_var_name = None;
    let settings = render_fish_settings(HOME, &opts);
    assert_eq!(
        settings,
        r"if not string match -q -- '/home/user/.pnpm' $PATH
  set -gx PATH '/home/user/.pnpm' $PATH
end",
    );
}

#[test]
fn nu_settings_with_proxy_var_sub_dir() {
    let mut opts = opts(false);
    opts.proxy_var_sub_dir = Some("bin");
    let settings = render_nu_settings(HOME, &opts);
    assert_eq!(
        settings,
        r#"$env.PNPM_HOME = "/home/user/.pnpm"
$env.PATH = ($env.PATH | split row (char esep) | prepend ($env.PNPM_HOME | path join "bin") )"#,
    );
}

#[test]
fn nu_settings_appending_to_the_end_of_path() {
    let mut opts = opts(false);
    opts.proxy_var_name = None;
    opts.position = AddingPosition::End;
    let settings = render_nu_settings(HOME, &opts);
    assert_eq!(
        settings,
        r#"$env.PATH = ($env.PATH | split row (char esep) | append "/home/user/.pnpm" )"#,
    );
}

#[test]
fn proxy_value_is_single_quote_escaped_against_injection() {
    // A command-substitution payload and an embedded single quote must end
    // up inside a single-quoted literal (the `'` escaped as `'\''`), so the
    // shell never expands `$(...)` when the rc file is sourced.
    let settings = render_posix_settings("/home/u/$(touch pwned)/it's", &opts(false));
    let first_line = settings.lines().next();
    assert_eq!(first_line, Some(r"export PNPM_HOME='/home/u/$(touch pwned)/it'\''s'"));
}

#[test]
fn create_config_file_when_it_does_not_exist() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_file = dir.path().join("sub").join(".bashrc");
    let content = wrap_settings("pnpm", "export FOO=1");

    let (change_type, old_settings) =
        update_shell_config(&config_file, &content, &opts(false)).expect("update shell config");

    assert_eq!(change_type, ConfigFileChangeType::Created);
    assert_eq!(old_settings, "");
    let written = fs::read_to_string(&config_file).expect("read config file");
    assert_eq!(written, "# pnpm\nexport FOO=1\n# pnpm end\n");
}

#[test]
fn append_to_an_existing_config_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_file = dir.path().join(".bashrc");
    fs::write(&config_file, "").expect("write empty config file");
    let content = wrap_settings("pnpm", "export FOO=1");

    let (change_type, old_settings) =
        update_shell_config(&config_file, &content, &opts(false)).expect("update shell config");

    assert_eq!(change_type, ConfigFileChangeType::Appended);
    assert_eq!(old_settings, "");
    let written = fs::read_to_string(&config_file).expect("read config file");
    assert_eq!(written, "\n# pnpm\nexport FOO=1\n# pnpm end\n");
}

#[test]
fn skip_when_the_config_is_already_present() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_file = dir.path().join(".bashrc");
    let content = wrap_settings("pnpm", "export FOO=1");
    fs::write(&config_file, format!("\n{content}\n")).expect("write config file");

    let (change_type, old_settings) =
        update_shell_config(&config_file, &content, &opts(false)).expect("update shell config");

    assert_eq!(change_type, ConfigFileChangeType::Skipped);
    assert_eq!(old_settings, "export FOO=1");
}

#[test]
fn fail_when_the_section_differs_and_overwrite_is_off() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_file = dir.path().join(".bashrc");
    let existing = wrap_settings("pnpm", "export FOO=old");
    fs::write(&config_file, format!("\n{existing}\n")).expect("write config file");
    let content = wrap_settings("pnpm", "export FOO=new");

    let err = update_shell_config(&config_file, &content, &opts(false))
        .expect_err("differing section without --force must error");
    assert!(matches!(err, PathExtenderError::BadShellSection { .. }));
    // The original content is left untouched.
    let written = fs::read_to_string(&config_file).expect("read config file");
    assert_eq!(written, format!("\n{existing}\n"));
}

#[test]
fn replace_when_the_section_differs_and_overwrite_is_on() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_file = dir.path().join(".bashrc");
    let existing = wrap_settings("pnpm", "export FOO=old");
    fs::write(&config_file, format!("before\n{existing}\nafter\n")).expect("write config file");
    let content = wrap_settings("pnpm", "export FOO=new");

    let (change_type, old_settings) =
        update_shell_config(&config_file, &content, &opts(true)).expect("update shell config");

    assert_eq!(change_type, ConfigFileChangeType::Modified);
    assert_eq!(old_settings, "export FOO=old");
    let written = fs::read_to_string(&config_file).expect("read config file");
    assert_eq!(written, format!("before\n{content}\nafter\n"));
}

#[test]
fn find_section_extracts_the_inner_settings() {
    let content = "head\n# pnpm\nbody line 1\nbody line 2\n# pnpm end\ntail";
    let (range, inner) = find_section(content, "pnpm").expect("section is present");
    assert_eq!(inner, "body line 1\nbody line 2");
    assert_eq!(&content[range], "# pnpm\nbody line 1\nbody line 2\n# pnpm end");
}

#[test]
fn find_section_absent_returns_none() {
    assert!(find_section("no markers here", "pnpm").is_none());
}

#[test]
fn replace_section_swaps_the_block() {
    let content = "a\n# pnpm\nold\n# pnpm end\nb";
    let replaced = replace_section(content, "# pnpm\nnew\n# pnpm end", "pnpm");
    assert_eq!(replaced, "a\n# pnpm\nnew\n# pnpm end\nb");
}
