//! Local git repositories for tests that install a git-hosted dependency.
//!
//! A test points its manifest at a repo on disk (`git+file://…`) rather
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

        git(&bare, &["init", "-q", "--bare"]);
        git(&work, &["init", "-q", "-b", "main"]);
        git(&work, &["config", "user.email", "test@example.invalid"]);
        git(&work, &["config", "user.name", "Test"]);
        // Neutralise a user-global `gpgsign = true`, which would
        // otherwise demand a real signing key for every commit and tag.
        git(&work, &["config", "commit.gpgsign", "false"]);
        git(&work, &["config", "tag.gpgsign", "false"]);
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
        // `dunce` keeps Windows paths in their `C:\…` form rather than
        // the `\\?\` UNC prefix, which `Url::from_file_path` would
        // otherwise carry into the URL and `git` would reject.
        let canonical = dunce::canonicalize(&self.bare).expect("canonicalize bare repo path");
        Url::from_file_path(&canonical)
            .unwrap_or_else(|()| panic!("build a file:// URL for {}", canonical.display()))
            .to_string()
    }

    /// The `git+file://…#<committish>` specifier a manifest declares to
    /// install this repo at `committish` (a SHA, tag, or branch).
    #[must_use]
    pub fn git_url_at(&self, committish: &str) -> String {
        format!("git+{}#{committish}", self.file_url())
    }
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
