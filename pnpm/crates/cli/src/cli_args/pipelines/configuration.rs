use super::{
    Config, Context, Host, InstallArgs, Path, PathBuf, ReporterType, read_manifest_json,
    reporter_emit, resolve_bool_override, warn_deprecated_override_version_references,
    warn_ignored_pnpm_manifest_fields, warn_unapplied_package_configs,
    warn_unmatched_registry_options, warn_unsupported_workspaces_field,
};
use crate::cli_args::config_warnings::emit_config_warning;
use miette::IntoDiagnostic;
use pnpm_config::WORKSPACE_MANIFEST_FILENAME;

/// [`select_workspace_projects`](super::select_workspace_projects), optionally running the install's
/// workspace-cycle search over the selection graph while it is still in
/// hand. Callers pass `true` only for a run that is certain to reach
/// the cycle check — the "Already up to date" fast path returns before
/// it, and a search it never reads would tax exactly that path.
/// `runtimeOnFail` is a workspace-level override the projects carry into
/// their own installs, so it is applied to each manifest as it is read.
pub(super) fn apply_runtime_on_fail(cfg: &Config, projects: &mut [pnpm_workspace::Project]) {
    let Some(runtime_on_fail) = cfg.runtime_on_fail else {
        return;
    };
    for project in projects {
        pnpm_package_manifest::apply_runtime_on_fail_override(
            project.manifest.value_mut(),
            runtime_on_fail.as_str(),
        );
    }
}

/// Shared workspace-root and package-manager policy derivation used by the
/// install, dedupe, and prune dispatch paths.
pub(crate) fn derive_config_root(
    cfg: &mut Config,
    dir_ref: &Path,
    reporter: ReporterType,
) -> miette::Result<PathBuf> {
    let config_root = cfg.root_project_manifest_dir(dir_ref).to_path_buf();
    let root_manifest = read_manifest_json(&config_root.join("package.json"))
        .wrap_err("read package manager policy")?;
    // pnpm warns from config-reading, so the notice lands ahead of any
    // install output. This is the install family's earliest point that
    // knows the root manifest's directory.
    warn_ignored_pnpm_manifest_fields(root_manifest.as_ref());
    create_workspace_yaml_from_yarn_workspaces(cfg, &config_root, root_manifest.as_ref())?;
    warn_deprecated_override_version_references(cfg, reporter_emit(reporter));
    warn_unmatched_registry_options(cfg);
    warn_unapplied_package_configs(cfg);
    Ok(config_root)
}

/// Create `pnpm-workspace.yaml` from a root manifest's Yarn `workspaces`
/// field, so the converted repository's projects link on this install.
///
/// The field is Yarn's and npm's way to declare a monorepo's projects;
/// pnpm reads `pnpm-workspace.yaml` instead, and a repository converted
/// without one installs as a single project with no hint about why. The
/// install family is the migration's entry point, so it writes the file
/// the field implies and re-anchors the workspace config to it. Inside an
/// existing workspace (`workspace_dir` set), under `--ignore-workspace`,
/// or when the field is absent, the config load is left untouched; a
/// non-array spelling keeps pnpm 11's quiet behavior, and an array with
/// no usable pattern keeps the unsupported-field warning.
fn create_workspace_yaml_from_yarn_workspaces(
    cfg: &mut Config,
    config_root: &Path,
    root_manifest: Option<&serde_json::Value>,
) -> miette::Result<()> {
    if cfg.workspace_dir.is_some() || cfg.ignore_workspace {
        return Ok(());
    }
    let Some(entries) = root_manifest
        .and_then(|manifest| manifest.get("workspaces"))
        .and_then(serde_json::Value::as_array)
    else {
        return Ok(());
    };
    let patterns: Vec<String> = entries
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter(|pattern| !pattern.is_empty())
        .map(str::to_owned)
        .collect();
    // An array whose entries are all unusable keeps pnpm 11's warning:
    // the field names projects pnpm cannot find, and the notice explains
    // why none of them linked.
    if patterns.is_empty() {
        if !entries.is_empty() {
            warn_unsupported_workspaces_field(root_manifest, None);
        }
        return Ok(());
    }
    let path = config_root.join(WORKSPACE_MANIFEST_FILENAME);
    // symlink_metadata, not try_exists: a manifest that is a symlink is a
    // file the repository authored, and following a dangling one would
    // write the generated YAML to the link's target instead.
    match std::fs::symlink_metadata(&path) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("check for an existing pnpm-workspace.yaml at {}", path.display())
                });
        }
    }
    // Render through the workspace-manifest writer so patterns that are
    // YAML syntax (a leading `!` or `#`, a newline) come out quoted and
    // the generated file re-parses to the declared patterns.
    let text = match pnpm_workspace_manifest_writer::edit_manifest_field(
        None,
        "packages",
        &serde_json::Value::Array(
            patterns
                .iter()
                .map(|pattern| serde_json::Value::String(pattern.clone()))
                .collect(),
        ),
    ) {
        Ok(pnpm_workspace_manifest_writer::ManifestEdit::Write(text)) => text,
        Ok(_) => return Ok(()),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("render pnpm-workspace.yaml for {}", path.display()));
        }
    };
    match publish_new_workspace_manifest(&path, &text) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // The manifest that won the race is the repository's now;
            // anchor this install to it instead of the single-package
            // fallback, exactly like one that was there before the check.
            let manifest = pnpm_workspace::read_workspace_manifest(config_root)
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("read the pnpm-workspace.yaml created at {}", path.display())
                })?;
            if let Some(manifest) = manifest {
                cfg.workspace_dir = Some(config_root.to_path_buf());
                cfg.workspace_package_patterns =
                    Some(pnpm_workspace::workspace_package_patterns(&manifest));
            }
            return Ok(());
        }
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("create pnpm-workspace.yaml at {}", path.display()));
        }
    }
    emit_config_warning(
        "Created \"pnpm-workspace.yaml\" from the \"workspaces\" field in package.json.",
    );
    cfg.workspace_dir = Some(config_root.to_path_buf());
    cfg.workspace_package_patterns = Some(patterns);
    Ok(())
}

/// Publish the generated manifest at `path` without ever exposing a
/// partial write and without ever replacing one that appears mid-flight:
/// the YAML lands in a sibling temp file, and a no-replace rename makes
/// it visible only once it is complete. Readers therefore see either no
/// manifest or a whole one, never a truncated or empty file.
///
/// `AlreadyExists` means another writer's manifest won the race, and the
/// caller anchors this install to it. A failed write cleans up only the
/// temp file it created (via the `TempFile` drop), never the published
/// path, so a file that replaced ours mid-failure survives untouched.
fn publish_new_workspace_manifest(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = tempfile::Builder::new()
        .prefix(".pnpm-workspace-")
        .suffix(".yaml-tmp")
        .tempfile_in(dir)?;
    // `NamedTempFile` starts 0600; the manifest is an ordinary project
    // file, so give it the 0644 a plain `File::create` would have left
    // under the common umask before this path went through a temp file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o644))?;
    }
    tmp.as_file()
        .write_all(text.as_bytes())
        .and_then(|()| tmp.as_file().flush())?;
    match tmp.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(persist) => Err(persist.error),
    }
}

pub(crate) fn apply_install_cli_config(cfg: &mut Config, args: &InstallArgs) {
    args.network_cache.apply(cfg);
    args.lockfile_updates.dedupe.apply(cfg);
    cfg.frozen_store = resolve_bool_override(
        args.materialization.frozen_store,
        args.materialization.no_frozen_store,
        cfg.frozen_store,
    );
    args.scripts.apply(cfg);
    cfg.force = args.materialization.force || cfg.force;
    if let Some(network_concurrency) = args.fetching.concurrency {
        cfg.network_concurrency = network_concurrency;
    }
    if let Some(fetch_timeout) = args.fetching.timeout {
        cfg.fetch_timeout = fetch_timeout;
    }
    if let Some(fetch_warn_timeout_ms) = args.fetching.warn_timeout_ms {
        cfg.fetch_warn_timeout_ms = fetch_warn_timeout_ms;
    }
    if let Some(fetch_min_speed_ki_bps) = args.fetching.min_speed_ki_bps {
        cfg.fetch_min_speed_ki_bps = fetch_min_speed_ki_bps;
    }
    if let Some(user_agent) = args.fetching.user_agent.clone() {
        cfg.user_agent = user_agent;
    }
    if let Some(pnpr_server) = args.fetching.pnpr_server.clone() {
        cfg.pnpr_server = Some(pnpr_server);
    }
    // pnpm merges its CLI options into the config *before* deciding
    // `mergeGitBranchLockfiles`, so a pattern given on the command line
    // still gets matched against the current branch — and an explicit
    // `--merge-git-branch-lockfiles` settles the question without it.
    if args.lockfile_updates.merge_git_branch_lockfiles {
        cfg.merge_git_branch_lockfiles = true;
    } else if !args.lockfile_updates.merge_git_branch_lockfiles_branch_pattern.is_empty() {
        cfg.merge_git_branch_lockfiles_branch_pattern.clone_from(
            &args.lockfile_updates.merge_git_branch_lockfiles_branch_pattern,
        );
        cfg.apply_git_branch_lockfile_derivation::<Host>();
    }
}

/// Whether the active directory has no manifest of its own and is none of
/// the workspace's projects, so the manifest at hand stands in for one.
pub(super) fn active_manifest_is_standin(
    active_dir: &Path,
    projects: &[pnpm_workspace::Project],
) -> miette::Result<bool> {
    let normalized_active_dir = pnpm_fs::lexical_normalize(active_dir);
    Ok(!active_dir.join("package.json").is_file()
        && pnpm_workspace::try_read_project_manifest(active_dir)
            .map_err(miette::Report::new)?
            .is_none()
        && !projects
            .iter()
            .any(|project| pnpm_fs::lexical_normalize(&project.root_dir) == normalized_active_dir))
}

#[cfg(test)]
mod publish_new_workspace_manifest_tests {
    use super::publish_new_workspace_manifest;
    use std::fs;

    #[test]
    fn publishes_the_full_text_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pnpm-workspace.yaml");

        publish_new_workspace_manifest(&path, "packages:\n  - packages/*\n")
            .expect("publish succeeds");

        assert_eq!(
            fs::read_to_string(&path).expect("manifest written"),
            "packages:\n  - packages/*\n",
        );
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != "pnpm-workspace.yaml")
            .collect();
        assert!(leftovers.is_empty(), "temp file must not survive: {leftovers:?}");
    }

    #[test]
    fn an_existing_manifest_wins_and_is_left_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pnpm-workspace.yaml");
        let authored = "packages:\n  - .\n";
        fs::write(&path, authored).expect("author manifest");

        let error = publish_new_workspace_manifest(&path, "packages:\n  - packages/*\n")
            .expect_err("publish must not clobber");

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).expect("manifest kept"), authored);
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != "pnpm-workspace.yaml")
            .collect();
        assert!(leftovers.is_empty(), "temp file must not survive: {leftovers:?}");
    }

    /// The loser of the race sees `AlreadyExists` the moment the winner's
    /// manifest is visible, and by then it is already complete: the
    /// no-replace rename is the manifest's first and only appearance.
    #[test]
    fn the_published_manifest_parses_from_the_first_visible_byte() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pnpm-workspace.yaml");
        let text = "packages:\n  - packages/*\n  - apps/web\n";

        publish_new_workspace_manifest(&path, text).expect("publish succeeds");

        let visible = fs::read_to_string(&path).expect("manifest visible");
        let parsed: serde_json::Value =
            serde_saphyr::from_str(&visible).expect("a reader that sees the file parses it");
        assert_eq!(parsed, serde_json::json!({"packages": ["packages/*", "apps/web"]}));
    }

    /// Two converting installs racing on the same repository: exactly one
    /// publishes, the loser gets `AlreadyExists` and anchors to the
    /// winner, and the manifest on disk is never a mix or a fragment of
    /// the two writers' texts. The loser also reads the file at the
    /// instant it loses: what it sees must already be the winner's
    /// complete manifest, never a partial one.
    #[test]
    fn concurrent_installs_publish_exactly_one_complete_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pnpm-workspace.yaml");
        let texts = ["packages:\n  - packages/*\n  - apps/web\n", "packages:\n  - crates/*\n"];

        let handles: Vec<_> = texts
            .iter()
            .map(|text| {
                let path = path.clone();
                let text = (*text).to_owned();
                std::thread::spawn(move || {
                    match publish_new_workspace_manifest(&path, &text) {
                        Ok(()) => (true, None),
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                            // Read at the instant of losing: this is what a
                            // concurrent install's anchor path would parse.
                            (false, Some(fs::read_to_string(&path).expect("loser can read")))
                        }
                        Err(error) => panic!("unexpected publish error: {error}"),
                    }
                })
            })
            .collect();
        // Collect-then-join, not a lazy chain: both publishers must be
        // in flight before either is joined, or there is no race.
        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            results.push(handle.join().expect("publisher thread"));
        }

        let published = results
            .iter()
            .filter(|(ok, _)| *ok)
            .count();
        assert_eq!(published, 1, "exactly one racer may publish: {results:?}");
        let (_, loser_view) = results
            .iter()
            .find(|(ok, _)| !*ok)
            .expect("one loser");
        let seen = loser_view.as_ref().expect("loser read the manifest");
        assert!(
            texts.contains(&seen.as_str()),
            "the loser must never observe a partial manifest, got: {seen:?}",
        );
        let visible = fs::read_to_string(&path).expect("winner's manifest visible");
        assert!(
            texts.contains(&visible.as_str()),
            "the manifest must be one racer's complete text, got: {visible:?}",
        );
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != "pnpm-workspace.yaml")
            .collect();
        assert!(leftovers.is_empty(), "temp file must not survive: {leftovers:?}");
    }
}
