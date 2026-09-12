use super::{BenchId, PNPM_BUNDLE_PATHS, WorkEnv, sync_bench_repo};
use crate::{cli_args::TargetKind, verify::executor};
use pipe_trait::Pipe;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

impl WorkEnv {
    /// Source-tree location for a pacquet revision: `<bench_dir>/pacquet`.
    fn pacquet_source_dir(&self, revision: &str) -> PathBuf {
        self.bench_dir(BenchId::PacquetRevision(revision)).join("pacquet")
    }
    /// Source-tree location for a pnpr revision: `<bench_dir>/pacquet`.
    /// A pnpr target builds from the same monorepo clone as a pacquet
    /// target (the `pacquet` and `pnpr` crates share one workspace), so
    /// the layout matches [`Self::pacquet_source_dir`].
    fn pnpr_source_dir(&self, revision: &str) -> PathBuf {
        self.bench_dir(BenchId::PnprRevision(revision)).join("pacquet")
    }
    /// Source-tree location for a pnpm revision: `<bench_dir>/pnpm-source`.
    fn pnpm_source_dir(&self, revision: &str) -> PathBuf {
        self.bench_dir(BenchId::PnpmRevision(revision)).join("pnpm-source")
    }
    pub(super) fn resolve_revision(repository: &Path, revision: &str) -> String {
        let output = Command::new("git")
            .current_dir(repository)
            .arg("rev-parse")
            .arg(revision)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .output()
            .expect("git rev-parse");
        assert!(output.status.success());
        output
            .stdout
            .pipe(String::from_utf8)
            .expect("output of rev-parse is valid UTF-8")
            .trim()
            .to_string()
    }
    /// Output binary a `pacquet@<rev>` target runs. Revisions disagree
    /// on the client bin name, so prefer whichever exists (`pnpm` when
    /// neither does yet).
    fn pacquet_binary(&self, revision: &str) -> PathBuf {
        WorkEnv::client_binary_in(&self.pacquet_source_dir(revision))
    }
    /// The client binary a `pnpr@<rev>` build produces (the pnpr
    /// target builds the client and `pnpr` bins together).
    fn pnpr_pacquet_binary(&self, revision: &str) -> PathBuf {
        WorkEnv::client_binary_in(&self.pnpr_source_dir(revision))
    }
    /// `<source_dir>/target/release/<client bin>` under either bin name:
    /// the existing one wins so prebuilt caches and older revisions keep
    /// working; `pnpm` is the default. The build fns delete the sibling
    /// name after every build, so at most one candidate exists.
    pub(super) fn client_binary_in(source_dir: &Path) -> PathBuf {
        let release = source_dir.join("target").join("release");
        let pnpm = release.join("pnpm");
        if pnpm.is_file() {
            return pnpm;
        }
        let pacquet = release.join("pacquet");
        if pacquet.is_file() { pacquet } else { pnpm }
    }
    /// CLI bin name the revision's checkout declares — `pnpm` on current
    /// revisions, `pacquet` on older ones. Read from the clone's CLI
    /// crate manifest (under either directory layout) so any revision
    /// builds with the right `--bin` flag. The probe is a line scan, not
    /// a TOML parse: the workspace has no TOML-parser dependency, and
    /// pulling one in for a single boolean probe isn't worth it.
    pub(super) fn cli_bin_name(revision_repo: &Path) -> &'static str {
        for dir in ["pnpm", "pacquet"] {
            let manifest = revision_repo.join(dir).join("crates").join("cli").join("Cargo.toml");
            if let Ok(content) = fs::read_to_string(&manifest) {
                // Whitespace-tolerant match: taplo pads `name` keys for
                // alignment in some tables.
                let has_pnpm_bin = content.lines().any(|line| {
                    let mut parts = line.split_whitespace();
                    parts.next() == Some("name")
                        && parts.next() == Some("=")
                        && parts.next() == Some(r#""pnpm""#)
                });
                return if has_pnpm_bin { "pnpm" } else { "pacquet" };
            }
        }
        "pnpm"
    }
    /// The `pnpr` server binary a `pnpr@<rev>` build produces.
    pub(super) fn pnpr_server_binary(&self, revision: &str) -> PathBuf {
        self.pnpr_source_dir(revision).join("target").join("release").join("pnpr")
    }
    /// Delete the client binary under the name `built_bin` does NOT use.
    /// A bench dir is keyed by the revision *label* (e.g. `pnpr@main`),
    /// so a moving label can leave the other name's binary from an
    /// earlier run in `target/release`; without this cleanup,
    /// existence-based resolution could pick that stale executable.
    fn remove_sibling_client_binary(source_dir: &Path, built_bin: &str) {
        let sibling = if built_bin == "pnpm" { "pacquet" } else { "pnpm" };
        let _ = fs::remove_file(source_dir.join("target").join("release").join(sibling));
    }
    pub fn build(&self) {
        eprintln!("Building...");
        // Build `pnpr@<rev>` targets first: a pnpr build also produces the
        // `pacquet` client binary, so a same-revision `pacquet@<rev>` can
        // reuse it (see [`Self::build_pacquet`]) instead of compiling the
        // identical commit a second time.
        let pnpr_first = self
            .targets
            .iter()
            .filter(|target| target.kind == TargetKind::Pnpr)
            .chain(self.targets.iter().filter(|target| target.kind != TargetKind::Pnpr));
        for target in pnpr_first {
            match target.kind {
                TargetKind::Pacquet => self.build_pacquet(&target.rev),
                TargetKind::Pnpm => self.build_pnpm(&target.rev),
                TargetKind::Pnpr => self.build_pnpr(&target.rev),
            }
        }
    }
    pub(super) fn build_pacquet(&self, revision: &str) {
        let dest = self.pacquet_binary(revision);

        // Restored from the per-commit CI binary cache: nothing to build.
        if self.reuse_prebuilt_binaries && dest.is_file() {
            eprintln!("Revision: {revision:?} (pacquet) — reusing prebuilt binary");
            return;
        }

        if self.reuse_pnpr_client_binary(revision) {
            return;
        }

        eprintln!("Revision: {revision:?} (pacquet)");

        let repository = self.repository();
        let revision_repo = self.pacquet_source_dir(revision);

        // Resolve the revision against the source repository *before*
        // fetching, so the fetch can request the exact commit. A bare
        // `git fetch <repo>` only writes the source's `HEAD` to
        // `FETCH_HEAD`, which means a SHA that isn't reachable from
        // the source's HEAD (e.g. tip of `main` when the runner is on
        // a PR branch that's behind `main`) won't end up in the
        // bench-repo and the subsequent `git checkout <sha>` panics
        // with `unable to read tree`. See PR <https://github.com/pnpm/pacquet/pull/321> comment
        // <https://github.com/pnpm/pacquet/pull/321#issuecomment-4326141435>.
        let commit = WorkEnv::resolve_revision(repository, revision);
        eprintln!("Resolved {revision:?} to {commit}");

        sync_bench_repo(repository, &revision_repo, &commit);

        eprintln!("Building {revision:?}...");
        let bin = WorkEnv::cli_bin_name(&revision_repo);
        Command::new("cargo")
            .current_dir(&revision_repo)
            .arg("build")
            .arg("--release")
            .arg(format!("--bin={bin}"))
            .pipe(executor("cargo build"));
        WorkEnv::remove_sibling_client_binary(&revision_repo, bin);
    }
    /// Reuse the client built by this revision's pnpr target, retaining its binary name.
    fn reuse_pnpr_client_binary(&self, revision: &str) -> bool {
        if self
            .targets
            .iter()
            .any(|target| target.kind == TargetKind::Pnpr && target.rev == revision)
        {
            let from_pnpr = self.pnpr_pacquet_binary(revision);
            if from_pnpr.is_file() {
                eprintln!(
                    "Revision: {revision:?} (pacquet) — reusing the binary from the pnpr@{revision} build",
                );
                // Name the copy after the source so the copied binary keeps
                // the bin name its revision declares.
                let bin_name = from_pnpr.file_name().expect("client binary path has a file name");
                let dest =
                    self.pacquet_source_dir(revision).join("target").join("release").join(bin_name);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent).expect("create pacquet target/release dir");
                }
                fs::copy(&from_pnpr, &dest).expect("copy the client binary from the pnpr build");
                let built = bin_name.to_str().expect("client bin name is UTF-8");
                WorkEnv::remove_sibling_client_binary(&self.pacquet_source_dir(revision), built);
                return true;
            }
        }

        false
    }
    /// Build a pnpr target: both the `pacquet` client and the `pnpr`
    /// server binaries from the revision's monorepo clone. The server is
    /// spawned later, at benchmark time, from
    /// `<bench_dir>/pacquet/target/release/pnpr`.
    fn build_pnpr(&self, revision: &str) {
        // Restored from the per-commit CI binary cache: nothing to build.
        if self.reuse_prebuilt_binaries
            && self.pnpr_pacquet_binary(revision).is_file()
            && self.pnpr_server_binary(revision).is_file()
        {
            eprintln!("Revision: {revision:?} (pnpr) — reusing prebuilt binaries");
            return;
        }

        eprintln!("Revision: {revision:?} (pnpr)");

        let repository = self.repository();
        let revision_repo = self.pnpr_source_dir(revision);

        let commit = WorkEnv::resolve_revision(repository, revision);
        eprintln!("Resolved {revision:?} to {commit}");

        sync_bench_repo(repository, &revision_repo, &commit);

        eprintln!("Building {revision:?} (client + pnpr)...");
        let bin = WorkEnv::cli_bin_name(&revision_repo);
        Command::new("cargo")
            .current_dir(&revision_repo)
            .arg("build")
            .arg("--release")
            .arg(format!("--bin={bin}"))
            .arg("--bin=pnpr")
            .pipe(executor("cargo build"));
        WorkEnv::remove_sibling_client_binary(&revision_repo, bin);
    }
    pub(super) fn build_pnpm(&self, revision: &str) {
        let revision_repo = self.pnpm_source_dir(revision);
        if self.reuse_prebuilt_binaries
            && PNPM_BUNDLE_PATHS.iter().any(|path| revision_repo.join(path).is_file())
        {
            eprintln!("Revision: {revision:?} (pnpm) — reusing prebuilt bundle");
            return;
        }

        eprintln!("Revision: {revision:?} (pnpm)");

        let repository = self.pnpm_repository();
        let commit = WorkEnv::resolve_revision(repository, revision);
        eprintln!("Resolved {revision:?} to {commit}");

        sync_bench_repo(repository, &revision_repo, &commit);

        eprintln!("Installing pnpm deps for {revision:?}...");
        Command::new("pnpm")
            .current_dir(&revision_repo)
            .arg("install")
            .pipe(executor("pnpm install"));

        eprintln!("Compiling pnpm for {revision:?}...");
        // `pnpm run compile-only` rather than `pnpm run compile` —
        // the root `compile` script also runs `update-manifests`,
        // which fires a second `pnpm install` and rewrites tracked
        // manifest files (a no-op for the benchmark, and the
        // rewrite-on-second-run was what made `sync_bench_repo`
        // need its `git reset --hard` guard). `compile-only` keeps
        // the workspace-manifest-reader / typecheck-only setup steps
        // *and* the final `pn -F=pnpm compile` that produces
        // `pnpm/dist/pnpm.{mjs,cjs}` — i.e. everything the install
        // script actually needs.
        Command::new("pnpm")
            .current_dir(&revision_repo)
            .arg("run")
            .arg("compile-only")
            .pipe(executor("pnpm run compile-only"));
    }
}
