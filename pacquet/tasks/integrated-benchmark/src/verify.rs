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
    }
}

/// Common git-repo check. Asserts `<path>/.git` exists as a directory
/// (a regular clone) or as a file whose contents begin with `gitdir:`
/// (a linked git worktree's pointer to the real gitdir). Validating
/// the file's contents rather than its mere existence avoids
/// misclassifying an arbitrary file named `.git` as a worktree
/// pointer.
fn ensure_git_repo_common(path: &Path) {
    assert!(path.is_dir(), "{path:?} is not a directory");
    let dot_git = path.join(".git");
    if dot_git.is_dir() {
        return;
    }
    if dot_git.is_file() {
        let contents = std::fs::read_to_string(&dot_git)
            .unwrap_or_else(|error| panic!("read {dot_git:?}: {error}"));
        assert!(
            contents.trim_start().starts_with("gitdir:"),
            "{path:?} has a `.git` file that is not a worktree gitdir pointer",
        );
        return;
    }
    panic!("{path:?} is not a git repository");
}

/// Assert that `path` is a git checkout of the pacquet codebase.
pub fn ensure_pacquet_git_repo(path: &Path) {
    ensure_git_repo_common(path);
    assert!(path.join("Cargo.toml").is_file(), "{path:?} has no Cargo.toml — pacquet checkout?");
    assert!(path.join("Cargo.lock").is_file(), "{path:?} has no Cargo.lock — pacquet checkout?");
}

/// Assert that `path` is a git checkout of pnpm's source — the
/// `pnpm/pnpm` monorepo. Looks for `pnpm/package.json` (the CLI
/// package's manifest, present in every revision since pnpm became a
/// monorepo) or `pnpm-workspace.yaml` at the root; either marker is
/// enough. Doesn't support a hypothetical layout where pnpm's source
/// lives directly at the repo root (no `pnpm/` subdir, no workspace
/// manifest) — that's not a shape pnpm ships in.
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
