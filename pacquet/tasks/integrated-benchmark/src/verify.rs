use std::{
    path::Path,
    process::{Command, Stdio},
};
use which::which;

pub async fn ensure_virtual_registry(registry: &str) {
    if let Err(error) = reqwest::Client::new().head(registry).send().await {
        eprintln!("HEAD request to {registry} returned an error");
        eprintln!("Make sure the registry server is operational");
        panic!("{error}");
    };
}

/// Common git-repo check. Asserts `<path>/.git` exists.
fn ensure_git_repo_common(path: &Path) {
    assert!(path.is_dir(), "{path:?} is not a directory");
    assert!(path.join(".git").is_dir(), "{path:?} is not a git repository");
}

/// Assert that `path` is a git checkout of the pacquet codebase.
pub fn ensure_pacquet_git_repo(path: &Path) {
    ensure_git_repo_common(path);
    assert!(path.join("Cargo.toml").is_file(), "{path:?} has no Cargo.toml — pacquet checkout?");
    assert!(path.join("Cargo.lock").is_file(), "{path:?} has no Cargo.lock — pacquet checkout?");
}

/// Assert that `path` is a git checkout of the pnpm codebase. Recognizes both
/// a standalone pnpm clone (top-level `pnpm/package.json`) and the
/// pnpm-as-monorepo layout this repo uses (a `pnpm-workspace.yaml` at the
/// root), so the same orchestrator binary works for both shapes.
pub fn ensure_pnpm_git_repo(path: &Path) {
    ensure_git_repo_common(path);
    let has_pnpm_dir = path.join("pnpm").join("package.json").is_file();
    let has_workspace_yaml = path.join("pnpm-workspace.yaml").is_file();
    assert!(
        has_pnpm_dir || has_workspace_yaml,
        "{path:?} doesn't look like a pnpm checkout — \
         expected `pnpm/package.json` or `pnpm-workspace.yaml` at the root",
    );
}

pub fn ensure_program(program: &str) -> Command {
    match which(program) {
        Ok(real_program) => Command::new(real_program),
        Err(which::Error::CannotFindBinaryPath) => panic!("Cannot find {program} in $PATH"),
        Err(error) => panic!("{error}"),
    }
}

pub fn validate_revision_list<List>(list: List)
where
    List: IntoIterator,
    List::Item: AsRef<str>,
{
    for revision in list {
        let revision = revision.as_ref();
        if revision.starts_with('.') {
            eprintln!("Revision {revision:?} is invalid");
            panic!("Revision cannot start with a dot");
        }
        // `@` is allowed so tag-with-suffix revisions like `v1.0.0@sha`
        // reach git intact. Git's reflog syntax (`HEAD@{1}`) needs `{`
        // and `}` which remain rejected — that's intentional, the bench
        // doesn't support reflog revisions and the curly braces are
        // shell-metacharacters we don't want to embed in bench-dir names.
        // `/` is also rejected: a revision like `origin/main` would
        // produce a nested bench-dir path (`pnpm@origin/main/`) — callers
        // should pre-resolve remote-tracking refs to a local branch or SHA.
        let invalid_char = revision.chars().find(|char| !matches!(char, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '+' | '.' | '~' | '^' | '@'));
        if let Some(char) = invalid_char {
            eprintln!("Revision {revision:?} is invalid");
            panic!("Invalid character: {char:?}");
        }
    }
}

pub fn executor<'a>(message: &'a str) -> impl FnOnce(&'a mut Command) {
    move |command| {
        let output = command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()
            .expect(message);
        assert!(output.status.success(), "Process exits with non-zero status: {message}");
    }
}
