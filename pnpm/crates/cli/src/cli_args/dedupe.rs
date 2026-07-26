use std::{
    io::Write,
    path::{Path, PathBuf},
};

use crate::{
    State,
    cli_args::{
        deps_tree::render::{
            TreeNode, blue_bright_underline, gray, green, plain, red, render_archy,
        },
        peers::peer_issues_warrant_warning,
    },
};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_lockfile::Lockfile;
use pacquet_package_manager::{
    ImporterDiffKey, Install, LockfileDiff, ProjectMutation, SnapshotDiff, diff_lockfiles,
};
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use tempfile::NamedTempFile;

#[derive(Debug, Args)]
pub struct DedupeArgs {
    #[clap(long)]
    pub check: bool,
}

impl DedupeArgs {
    /// Run the deduplication install pipeline. In `--check` mode the method
    /// receives a pre-computed snapshot (`existing`) and drop guard created by
    /// the caller *before* config-dependency steps, so the gate covers any
    /// lockfile mutations made by config-deps as well.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        existing: Option<String>,
        guard: Option<LockfileGuard>,
        lockfile_path: &Path,
    ) -> miette::Result<()> {
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;

        Install {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            emit_initial_manifest: true,
            lockfile: pacquet_lockfile::MaybeLazyLockfile::Lazy(lockfile),
            lockfile_path: Some(lockfile_path),
            dependency_groups: [
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ]
            .into_iter(),
            frozen_lockfile: false,
            prefer_frozen_lockfile: Some(false),
            ignore_manifest_check: false,
            skip_runtimes: false,
            trust_lockfile: config.trust_lockfile,
            update_checksums: false,
            mutation: ProjectMutation::InstallWorkspace,
            installs_only: true,
            resolved_packages,
            supported_architectures: config.supported_architectures.clone(),
            node_linker: config.node_linker,
            lockfile_only: true,
            dry_run: false,
            update_seed_policy: pacquet_package_manager::UpdateSeedPolicy::drop_all(),
            auth_override: None,
            resolution_observer: None,
            peer_issues_sink: None,
            catalogs_override: None,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
        }
        .run::<Reporter>()
        .await
        .wrap_err("deduplicating dependencies")?;

        let current = read_lockfile_snapshot(lockfile_path)?;
        let deduped = parse_snapshot(current.as_deref(), lockfile_path);

        let lockfile_dir = lockfile_path.parent().unwrap_or_else(|| Path::new("."));
        if let Some(deduped) = &deduped
            && peer_issues_warrant_warning(deduped, lockfile_dir, &config.peer_dependency_rules)
        {
            Reporter::emit(&LogEvent::Global(GlobalLog {
                level: LogLevel::Warn,
                message:
                    r#"Issues with peer dependencies found. Run "pnpm peers check" to list them."#
                        .to_string(),
            }));
        }

        if self.check {
            let mut guard = guard.unwrap();
            if existing == current {
                guard.disarm();
                Ok(())
            } else {
                let report = render_dedupe_check_issues(&diff_lockfiles(
                    parse_snapshot(existing.as_deref(), lockfile_path).as_ref(),
                    deduped.as_ref(),
                    ImporterDiffKey::Version,
                ));
                eprintln!("{report}");
                Err(DedupeError::CheckIssues.into())
            }
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Display, Error, Diagnostic)]
enum DedupeError {
    #[display("Dedupe --check found changes to the lockfile")]
    #[diagnostic(
        code(ERR_PNPM_DEDUPE_CHECK_ISSUES),
        help("Run `pnpm dedupe` to apply the changes above.")
    )]
    CheckIssues,
}

/// Parse one side of the `--check` diff. A snapshot that does not parse —
/// an older lockfile format the dedupe install has just rewritten, say —
/// yields no baseline rather than replacing the check's verdict with a
/// parse error: the run already knows the lockfile would change, and only
/// the detail of the report is lost.
fn parse_snapshot(content: Option<&str>, lockfile_path: &Path) -> Option<Lockfile> {
    content.and_then(|content| Lockfile::parse(content, lockfile_path).ok().flatten())
}

/// Render what `pnpm dedupe` would rewrite, mirroring pnpm's
/// `renderDedupeCheckIssues`: one tree per changed importer or package
/// snapshot, plus the snapshots deduplication would add or drop.
///
/// The lockfile can also be rewritten without any resolution changing —
/// recorded settings drift, a config dependency the run synced — so an
/// empty diff still says why the check failed.
fn render_dedupe_check_issues(diff: &LockfileDiff) -> String {
    if diff.is_empty() {
        return "The lockfile would be rewritten, but no dependency resolution would change."
            .to_string();
    }
    [
        render_section("Importers", &diff.importers, &[], &[]),
        render_section(
            "Packages",
            &diff.updated_packages,
            &diff.added_packages,
            &diff.removed_packages,
        ),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n")
}

fn render_section(
    title: &str,
    updated: &[SnapshotDiff],
    added: &[String],
    removed: &[String],
) -> Option<String> {
    let mut lines: Vec<String> = updated.iter().map(render_snapshot_diff).collect();
    lines.extend(added.iter().map(|id| format!("{} {}", green("+"), plain(id))));
    lines.extend(removed.iter().map(|id| format!("{} {}", red("-"), plain(id))));
    if lines.is_empty() {
        return None;
    }
    Some(format!("{}\n{}\n", blue_bright_underline(title), lines.join("\n")))
}

fn render_snapshot_diff(diff: &SnapshotDiff) -> String {
    let added = diff
        .added
        .iter()
        .map(|(alias, next)| format!("{} {} {}", green("+"), plain(alias), gray(next)));
    let removed = diff
        .removed
        .iter()
        .map(|(alias, prev)| format!("{} {} {}", red("-"), plain(alias), gray(prev)));
    let updated = diff.updated.iter().map(|(alias, prev, next)| {
        format!("{} {} {} {}", plain(alias), red(prev), gray("→"), green(next))
    });
    let nodes = added
        .chain(removed)
        .chain(updated)
        .map(|label| TreeNode::with_children(label, Vec::new()))
        .collect();
    render_archy(&TreeNode::with_children(plain(&diff.id), nodes))
        .trim_end_matches('\n')
        .to_string()
}

/// Atomically write `content` to `path` via temp-file + rename, so the write
/// does not follow symlinks and cannot produce a torn file on crash.
fn atomic_write(path: &Path, content: &[u8]) -> miette::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = NamedTempFile::new_in(dir)
        .into_diagnostic()
        .wrap_err("creating temp file for atomic write")?;
    tmp.write_all(content).into_diagnostic().wrap_err("writing temp file")?;
    tmp.as_file().sync_all().into_diagnostic().wrap_err("syncing temp file")?;
    tmp.persist(path).into_diagnostic().wrap_err("renaming temp file into place")?;
    Ok(())
}

/// A drop guard for `--check` mode: restores the lockfile snapshot on drop
/// unless [`disarm`](LockfileGuard::disarm) has been called. This way an
/// unexpected error during deduplication still leaves the workspace in its
/// original state.
pub(crate) struct LockfileGuard {
    existing: Option<String>,
    lockfile_path: PathBuf,
    disarmed: bool,
}

impl LockfileGuard {
    pub(crate) fn new(existing: Option<String>, lockfile_path: &Path) -> Self {
        Self { existing, lockfile_path: lockfile_path.to_path_buf(), disarmed: false }
    }

    pub(crate) fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl Drop for LockfileGuard {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }
        match self.existing.take() {
            Some(ref old) => {
                let _ = atomic_write(&self.lockfile_path, old.as_bytes());
            }
            None => {
                let _ = std::fs::remove_file(&self.lockfile_path);
            }
        }
    }
}

/// Read pnpm-lock.yaml into an `Option<String>` for snapshot comparisons.
/// Returns `None` when the file does not exist.
pub(crate) fn read_lockfile_snapshot(lockfile_path: &Path) -> miette::Result<Option<String>> {
    match std::fs::read_to_string(lockfile_path) {
        Ok(content) => Ok(Some(content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).into_diagnostic().wrap_err("reading lockfile"),
    }
}
