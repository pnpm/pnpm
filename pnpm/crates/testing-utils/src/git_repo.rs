//! Local git repositories for tests that install a git-hosted dependency.
//!
//! A test points its manifest at a repo on disk (`git+file://...`) rather
//! than a real forge, so the whole git install path — `ls-remote`
//! resolution, clone, `prepare`, packlist, link — runs without network
//! access. The TypeScript suite uses the same technique for its
//! `prepare`-script coverage (`createGitPreparePackage` in
//! `installing/deps-installer/test/install/lifecycleScripts.ts`).
//!
//! Repos that need a *host* identity (a `codeload.github.com` archive
//! URL, `gitHosted: true`) can't be modeled this way — those resolve to
//! a tarball and are covered at the resolver level instead.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use url::Url;

/// A git work tree plus the bare clone a test's manifest points at.
///
/// Every [`Self::commit`] and [`Self::tag`] mirrors the work tree into
/// the bare repo, so the URL accessors always describe current history.
pub struct GitRepoFixture {
    work: PathBuf,
    bare: PathBuf,
}

impl GitRepoFixture {
    /// Create an empty repo under `root`, as a work tree at
    /// `<root>/<name>-src` and a bare clone at `<root>/<name>.git`.
    ///
    /// Put `root` outside the project being installed into — a git repo
    /// nested inside the workspace would be picked up as part of it.
    #[must_use]
    pub fn init(root: &Path, name: &str) -> Self {
        let work = root.join(format!("{name}-src"));
        let bare = root.join(format!("{name}.git"));
        fs::create_dir_all(&work).expect("create git work tree");
        fs::create_dir_all(&bare).expect("create bare repo directory");

        git(&bare, &["init", "-q", "--bare", "-b", "main", "--template="]);
        override_global_config(&bare, &bare);
        git(&work, &["init", "-q", "-b", "main", "--template="]);
        git(&work, &["config", "user.email", "test@example.invalid"]);
        git(&work, &["config", "user.name", "Test"]);
        override_global_config(&work, &work.join(".git"));
        git(&work, &["remote", "add", "origin", &bare.to_string_lossy()]);

        Self { work, bare }
    }

    /// Write `contents` to `relative_path` in the work tree, creating
    /// parent directories. Not committed until [`Self::commit`] runs.
    pub fn write_file(&self, relative_path: &str, contents: &str) {
        let path = self.work.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent directory");
        }
        fs::write(&path, contents).unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
    }

    /// Create a symlink at `relative_path` pointing at `target`, which
    /// is interpreted relative to the link's own directory the way git
    /// records one. Not committed until [`Self::commit`] runs.
    ///
    /// Unix only: Windows needs a privilege ordinary test runs do not
    /// have, so a test that checks symlink handling gates on the target
    /// family rather than probing for one.
    #[cfg(unix)]
    pub fn write_symlink(&self, relative_path: &str, target: &str) {
        let path = self.work.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent directory");
        }
        std::os::unix::fs::symlink(target, &path)
            .unwrap_or_else(|err| panic!("link {}: {err}", path.display()));
    }

    /// Stage every change, commit it, mirror to the bare repo, and
    /// return the new commit's SHA.
    #[must_use]
    pub fn commit(&self, message: &str) -> String {
        git(&self.work, &["add", "-A"]);
        git(&self.work, &["commit", "-q", "-m", message]);
        self.mirror();
        self.head()
    }

    /// Create an annotated tag on `HEAD` and mirror it to the bare repo.
    pub fn tag(&self, name: &str) {
        git(&self.work, &["tag", "-a", name, "-m", name]);
        self.mirror();
    }

    /// Force-push every branch and tag into the bare repo, so what a
    /// test resolves against always matches the work tree.
    fn mirror(&self) {
        git(&self.work, &["push", "-q", "--force", "origin", "--all"]);
        git(&self.work, &["push", "-q", "--force", "origin", "--tags"]);
    }

    /// SHA of the work tree's current `HEAD`.
    #[must_use]
    pub fn head(&self) -> String {
        git(&self.work, &["rev-parse", "HEAD"]).trim().to_string()
    }

    /// `file://` URL of the bare repo — the transport half of a git
    /// specifier, and the `repo` field a `type: git` resolution records.
    #[must_use]
    pub fn file_url(&self) -> String {
        // `dunce` keeps Windows paths in their `C:\...` form rather than
        // the `\\?\` UNC prefix, which `Url::from_file_path` would
        // otherwise carry into the URL and `git` would reject.
        let canonical = dunce::canonicalize(&self.bare).expect("canonicalize bare repo path");
        Url::from_file_path(&canonical)
            .unwrap_or_else(|()| panic!("build a file:// URL for {}", canonical.display()))
            .to_string()
    }

    /// The `git+file://...#<committish>` specifier a manifest declares to
    /// install this repo at `committish` (a SHA, tag, or branch).
    #[must_use]
    pub fn git_url_at(&self, committish: &str) -> String {
        format!("git+{}#{committish}", self.file_url())
    }
}

/// `git init` a repository at `path` on branch `main`, whatever the
/// contributor's `init.defaultBranch`, for a test that needs a repo
/// without the work tree and bare clone [`GitRepoFixture`] pairs up.
///
/// Overrides the user-global `core.excludesFile`, `core.attributesFile`,
/// `core.hooksPath`, `core.fsmonitor`, and `gpgsign` settings and skips
/// the user-global `init.templateDir`, so a contributor's own git
/// configuration cannot change what the repo ignores, what it runs on
/// staging and commit, or whether it demands a signing key.
/// Configuration beyond those still reaches it.
pub fn init_isolated_repo(path: &Path) {
    fs::create_dir_all(path).expect("create git repo directory");
    git(path, &["init", "-q", "-b", "main", "--template="]);
    git(path, &["config", "user.email", "test@example.invalid"]);
    git(path, &["config", "user.name", "Test"]);
    override_global_config(path, &path.join(".git"));
}

/// The path of every file in `repo` that git does not ignore, tracked or
/// not, as `git ls-files --cached --others --exclude-standard` lists them.
///
/// A test that asserts on pnpm's cache keys can check its own premise
/// with this: pnpm derives a task's inputs from the same listing, so a
/// fixture file missing here is a file the cache key cannot see.
#[must_use]
pub fn unignored_files(repo: &Path) -> Vec<String> {
    git(repo, &["ls-files", "--cached", "--others", "--exclude-standard"])
        .lines()
        .map(str::to_string)
        .collect()
}

/// Override, in the local configuration of the repo at `repo` whose git
/// directory is `git_dir`, the user-global settings that would otherwise
/// change what a fixture repo does: `core.excludesFile`,
/// `core.attributesFile`, `core.hooksPath`, `core.fsmonitor`, and
/// `gpgsign`. Configuration this does not name still reaches the repo.
///
/// `git ls-files --exclude-standard` consults the user-global excludes
/// file, and pnpm builds a task's cache inputs from that listing. A
/// contributor who ignores one of a fixture's file names globally would
/// otherwise watch the file drop out of the hashed inputs, and the test
/// asserting that editing it invalidates the task would fail on their
/// machine alone. Local configuration also covers the `git` that pnpm
/// itself spawns inside the repo, not just the fixture's own calls.
///
/// A bare repo needs this too: `git push` runs the receiving side's
/// `pre-receive` and `update` hooks from that repo's `core.hooksPath`.
///
/// The repo must have been created with `git init --template=`, since
/// a user-global `init.templateDir` would otherwise seed `info/exclude`,
/// which no configuration setting overrides.
fn override_global_config(repo: &Path, git_dir: &Path) {
    // A path that does not exist: git reads a missing excludes or
    // attributes file as empty, and a missing hooks directory as no
    // hooks. `/dev/null` would not work on Windows.
    let absent = git_dir.join("absent-global-config");
    let absent = absent.to_string_lossy();
    git(repo, &["config", "core.excludesFile", &absent]);
    // User-global attributes can assign a `clean` filter to a fixture's
    // files, a user-global `core.hooksPath` its own hooks, and a
    // user-global `core.fsmonitor` a command git consults whenever it
    // refreshes the index: each runs the contributor's arbitrary code on
    // `git add` and commit.
    git(repo, &["config", "core.attributesFile", &absent]);
    git(repo, &["config", "core.hooksPath", &absent]);
    git(repo, &["config", "core.fsmonitor", "false"]);
    // Neutralise a user-global `gpgsign = true`, which would
    // otherwise demand a real signing key for every commit and tag.
    git(repo, &["config", "commit.gpgsign", "false"]);
    git(repo, &["config", "tag.gpgsign", "false"]);
}

/// Run `git` with `args` in `cwd` and return its stdout.
///
/// Panics when `git` is missing or the command fails — per
/// `pnpm/AGENTS.md`, a test must not tolerate an under-provisioned
/// environment by skipping.
fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("run `git {}`: {err}", args.join(" ")));
    assert!(
        output.status.success(),
        "`git {}` failed in {}:\n{}",
        args.join(" "),
        cwd.display(),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8(output.stdout).expect("git stdout is UTF-8")
}

/// A forwarding Git wrapper that records acquisition commands without changing
/// process-global PATH. CLI tests prepend its directory only on the child.
#[cfg(unix)]
pub struct GitCommandLog {
    pub bin: PathBuf,
    log: PathBuf,
}

#[cfg(unix)]
impl GitCommandLog {
    #[must_use]
    pub fn new(root: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let bin_dir = root.join("git-wrapper");
        fs::create_dir_all(&bin_dir).unwrap();
        let bin = bin_dir.join("git");
        let log = bin_dir.join("acquisitions.log");
        let output = Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
        assert!(output.status.success(), "locate git: {output:?}");
        let git = String::from_utf8(output.stdout).unwrap();
        fs::write(bin_dir.join("real-git"), git.trim()).unwrap();
        let script = r#"#!/bin/sh
wrapper_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
real_git=$(cat "$wrapper_dir/real-git")
case "$1" in
clone) printf '%s\n' "$4" >> "$wrapper_dir/acquisitions.log";;
fetch) printf '%s\n' "$PWD" >> "$wrapper_dir/acquisitions.log";;
esac
exec "$real_git" "$@"
"#;
        fs::write(&bin, script).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(&log, "").unwrap();
        Self { bin, log }
    }

    #[must_use]
    pub fn acquisitions(&self) -> Vec<PathBuf> {
        fs::read_to_string(&self.log).unwrap().lines().map(PathBuf::from).collect()
    }
}
