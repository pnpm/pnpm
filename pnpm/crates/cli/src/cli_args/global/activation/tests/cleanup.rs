use super::{
    ACTIVATION_CALLS, ARTIFACT_PROBE_CALLS, ActivationFixture, Arc, ArtifactProbeFailure,
    BACKUP_CLEANUP_HASH_CALLS, BackupCleanupFailure, GlobalInstallCleanup, GlobalPackageInfo,
    GlobalRemovalTransaction, GlobalShims, HASH_FAILURE_CALLS, HashSet, HashSwapFailure, Host,
    Ordering, PackageBinSource, RENAME_FAILURE_HASH_CALLS, RenameRollbackFailure,
    TrackingActivation, activate_global_install, activate_global_install_with_extra_bin_names,
    arm_backup_cleanup_blocker, backup_cleanup_guard, backup_dirs, canonical,
    cleanup_replaced_global_installs, diagnostic_source_messages, force_symlink_dir, fs,
    global_package_with_bins, hash_failure_guard, io, json, plan_replaced_global_bins,
    record_virtual_shim_state, remove_global_install_entries, resolved_hash_target,
    restore_virtual_shims, snapshot_global_package, test_link_bins, virtual_shim_owner,
};

#[test]
fn unsupported_bin_slot_fails_before_activation_and_cleans_preparation_artifacts() {
    ACTIVATION_CALLS.store(0, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["tool"]);
    let unsupported_path = fixture.global_bin_dir.join("tool");
    fs::create_dir(&unsupported_path).expect("create unsupported bin directory");

    let error = activate_global_install::<TrackingActivation>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            test_link_bins::<TrackingActivation>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::new(),
            )
        },
    )
    .expect_err("a directory bin slot must be rejected during preparation");

    let diagnostic: &(dyn miette::Diagnostic + Send + Sync) = error.as_ref();
    assert_eq!(
        miette::Diagnostic::code(diagnostic).map(|code| code.to_string()),
        Some("ERR_PNPM_GLOBAL_BIN_UNSUPPORTED_TYPE".to_string()),
    );
    assert!(error.to_string().contains(&unsupported_path.display().to_string()));
    assert_eq!(ACTIVATION_CALLS.load(Ordering::SeqCst), 0);
    assert!(unsupported_path.is_dir());
    assert_eq!(resolved_hash_target(&fixture.hash_link), canonical(&fixture.old_install_dir));
    assert!(!fixture.fresh_install_dir.exists());
    assert!(backup_dirs(&fixture.global_bin_dir).is_empty());
}

#[test]
fn cleanup_after_activation_preserves_current_state_and_external_install() {
    let root = tempfile::tempdir().expect("create cleanup fixture");
    let global_pkg_dir = root.path().join("global");
    let global_bin_dir = root.path().join("bin");
    let old_install_dir = global_pkg_dir.join("old-install");
    let active_install_dir = global_pkg_dir.join("active-install");
    let external_install_dir = root.path().join("external-install");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    fs::create_dir_all(&active_install_dir).expect("create active install directory");
    fs::create_dir_all(&external_install_dir).expect("create external install directory");
    let active_group = global_package_with_bins(
        &old_install_dir,
        "active-hash",
        &["activated", "survivor", "obsolete"],
    );
    let external_group = snapshot_global_package(GlobalPackageInfo {
        hash: "external-hash".to_string(),
        install_dir: external_install_dir.clone(),
        dependencies: Vec::new(),
    })
    .expect("snapshot the external group's bin ownership");
    for bin_name in ["activated", "survivor", "obsolete"] {
        fs::write(global_bin_dir.join(bin_name), b"bin\n").expect("seed global bin");
    }
    let active_hash_link = pnpm_global::get_hash_link(&global_pkg_dir, "active-hash");
    force_symlink_dir(&active_install_dir, &active_hash_link).expect("seed active hash link");

    let leftover = cleanup_replaced_global_installs(
        &global_pkg_dir,
        &global_bin_dir,
        &[active_group, external_group],
        "active-hash",
        &HashSet::from(["activated".to_string()]),
        &HashSet::from(["survivor".to_string()]),
        &HashSet::new(),
    )
    .expect("clean up replaced installs after activation");
    assert!(leftover.is_none());

    assert!(global_bin_dir.join("activated").exists());
    assert!(global_bin_dir.join("survivor").exists());
    assert!(!global_bin_dir.join("obsolete").exists());
    assert_eq!(
        fs::canonicalize(&active_hash_link).expect("resolve active hash link"),
        fs::canonicalize(&active_install_dir).expect("resolve active install directory"),
    );
    assert!(!old_install_dir.exists());
    assert!(external_install_dir.exists());
}

#[test]
fn replacing_a_group_with_a_different_package_set_keeps_its_relinked_bins() {
    let root = tempfile::tempdir().expect("create cleanup fixture");
    let global_pkg_dir = root.path().join("global");
    let global_bin_dir = root.path().join("bin");
    let old_install_dir = global_pkg_dir.join("old-install");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    // `shared` is provided by the replaced group and by the group that just
    // took its place; `dropped` only by the replaced one.
    let replaced = global_package_with_bins(&old_install_dir, "old-hash", &["shared", "dropped"]);
    for bin_name in ["shared", "dropped"] {
        fs::write(global_bin_dir.join(bin_name), b"bin\n").expect("seed global bin");
    }
    let old_hash_link = pnpm_global::get_hash_link(&global_pkg_dir, "old-hash");
    force_symlink_dir(&old_install_dir, &old_hash_link).expect("seed old hash link");

    let leftover = cleanup_replaced_global_installs(
        &global_pkg_dir,
        &global_bin_dir,
        &[replaced],
        "new-hash",
        &HashSet::from(["shared".to_string()]),
        &HashSet::new(),
        &HashSet::new(),
    )
    .expect("clean up the replaced group");
    assert!(leftover.is_none());

    // Changing the set of packages changes the hash, so `shared` was
    // rewritten to point at the new one just before this ran — unlinking the
    // group it used to belong to must not take it away again.
    assert!(global_bin_dir.join("shared").exists());
    assert!(!global_bin_dir.join("dropped").exists());
    assert_eq!(
        fs::symlink_metadata(&old_hash_link).expect_err("the old hash link is gone").kind(),
        io::ErrorKind::NotFound,
    );
    assert!(!old_install_dir.exists());
}

#[test]
fn cleanup_failure_preserves_every_bin_and_the_group() {
    let root = tempfile::tempdir().expect("create cleanup error fixture");
    let global_pkg_dir = root.path().join("global");
    let global_bin_dir = root.path().join("bin");
    let install_dir = global_pkg_dir.join("old-install");
    fs::create_dir_all(global_bin_dir.join("blocked")).expect("create blocked bin directory");
    let group = global_package_with_bins(&install_dir, "active-hash", &["blocked", "stale"]);
    fs::write(global_bin_dir.join("stale"), b"stale\n").expect("seed stale bin");

    let error = cleanup_replaced_global_installs(
        &global_pkg_dir,
        &global_bin_dir,
        &[group],
        "active-hash",
        &HashSet::new(),
        &HashSet::new(),
        &HashSet::new(),
    )
    .expect_err("a directory cannot be removed as a bin file");

    let blocked_bin = global_bin_dir.join("blocked");
    assert!(error.to_string().contains("Cannot replace global bin slot"));
    assert!(error.to_string().contains(&blocked_bin.display().to_string()));
    assert!(global_bin_dir.join("stale").exists());
    assert!(blocked_bin.exists());
    assert!(install_dir.exists());
}

#[test]
fn replacing_a_package_that_drops_a_bin_restores_its_recorded_shim() {
    let root = tempfile::tempdir().expect("create replacement fixture");
    let global_pkg_dir = root.path().join("global");
    let global_bin_dir = root.path().join("bin");
    let install_dir = global_pkg_dir.join("old-install");
    let fresh_install_dir = global_pkg_dir.join("fresh-install");
    let package_dir = install_dir.join("node_modules/node");
    fs::create_dir_all(&package_dir).expect("create installed package directory");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    fs::write(
        install_dir.join("package.json"),
        serde_json::to_vec(&json!({ "dependencies": { "node": "1.0.0" } }))
            .expect("serialize global group manifest"),
    )
    .expect("write global group manifest");
    fs::write(
        package_dir.join("package.json"),
        serde_json::to_vec(&json!({
            "name": "node",
            "version": "1.0.0",
            "bin": { "node": "node.js" },
        }))
        .expect("serialize installed package manifest"),
    )
    .expect("write installed package manifest");
    fs::write(package_dir.join("node.js"), b"").expect("write package bin");
    fs::write(global_bin_dir.join("node"), b"old global bin\n").expect("seed global bin");
    record_virtual_shim_state(&global_bin_dir, "node", &["node".to_string()])
        .expect("record shim restoration state");
    let group = snapshot_global_package(GlobalPackageInfo {
        hash: "old-hash".to_string(),
        install_dir: install_dir.clone(),
        dependencies: vec![("node".to_string(), "1.0.0".to_string())],
    })
    .expect("snapshot the replaced group's bin ownership");
    let old_hash_link = pnpm_global::get_hash_link(&global_pkg_dir, "old-hash");
    force_symlink_dir(&install_dir, &old_hash_link).expect("seed old hash link");
    fs::create_dir_all(&fresh_install_dir).expect("create fresh install directory");

    let plan = plan_replaced_global_bins(
        std::slice::from_ref(&group),
        &global_bin_dir,
        &HashSet::new(),
        &HashSet::new(),
        &GlobalShims::default(),
    )
    .expect("plan dropped-bin replacement");
    let activation = activate_global_install_with_extra_bin_names::<Host>(
        &fresh_install_dir,
        &old_hash_link,
        &global_bin_dir,
        &[],
        &HashSet::new(),
        &plan.affected_bin_names,
        || restore_virtual_shims(&plan.shims_to_restore, &global_bin_dir),
    )
    .expect("activate replacement");
    assert!(activation.activated_bins.is_empty());

    let leftover = cleanup_replaced_global_installs(
        &global_pkg_dir,
        &global_bin_dir,
        &[group],
        "old-hash",
        &HashSet::new(),
        &HashSet::new(),
        &plan.restored_bin_names(),
    )
    .expect("clean up replaced package");

    assert!(leftover.is_none());
    assert_eq!(
        virtual_shim_owner(&global_bin_dir.join("node")).expect("inspect restored shim").as_deref(),
        Some("node"),
    );
    assert_eq!(resolved_hash_target(&old_hash_link), canonical(&fresh_install_dir));
    assert!(!install_dir.exists());
}

#[test]
fn dropped_bin_failure_restores_its_command_and_hash_target() {
    let root = tempfile::tempdir().expect("create dropped-bin rollback fixture");
    let global_bin_dir = root.path().join("bin");
    let old_install_dir = root.path().join("old-install");
    let fresh_install_dir = root.path().join("fresh-install");
    let hash_link = root.path().join("hash-link");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    fs::create_dir_all(&old_install_dir).expect("create old install directory");
    fs::create_dir_all(&fresh_install_dir).expect("create fresh install directory");
    fs::write(global_bin_dir.join("dropped"), b"old command\n").expect("seed old command");
    force_symlink_dir(&old_install_dir, &hash_link).expect("seed old hash link");

    let error = activate_global_install_with_extra_bin_names::<Host>(
        &fresh_install_dir,
        &hash_link,
        &global_bin_dir,
        &[],
        &HashSet::new(),
        &HashSet::from(["dropped".to_string()]),
        || Err(miette::miette!("injected restoration failure")),
    )
    .expect_err("restoration must fail");

    assert!(format!("{error:?}").contains("injected restoration failure"));
    assert_eq!(resolved_hash_target(&hash_link), canonical(&old_install_dir));
    assert_eq!(
        fs::read(global_bin_dir.join("dropped")).expect("read restored command"),
        b"old command\n",
    );
    assert!(!fresh_install_dir.exists());
}

#[test]
fn global_removal_reports_cleanup_failure_and_keeps_the_group() {
    let root = tempfile::tempdir().expect("create global removal fixture");
    let global_pkg_dir = root.path().join("global");
    let global_bin_dir = root.path().join("bin");
    let install_dir = global_pkg_dir.join("install");
    fs::create_dir_all(global_bin_dir.join("blocked")).expect("create blocked bin directory");
    let group = global_package_with_bins(&install_dir, "group-hash", &["blocked", "stale"]);
    fs::write(global_bin_dir.join("stale"), b"stale\n").expect("seed stale bin");
    let hash_link = pnpm_global::get_hash_link(&global_pkg_dir, "group-hash");
    force_symlink_dir(&install_dir, &hash_link).expect("seed global hash link");
    let bins_to_keep = HashSet::new();
    let cleanup = GlobalInstallCleanup {
        global_pkg_dir: &global_pkg_dir,
        global_bin_dir: &global_bin_dir,
        bins_to_keep: &bins_to_keep,
        hash_to_keep: None,
        context: "global",
    };
    let affected_bin_names = HashSet::from(["blocked".to_string(), "stale".to_string()]);
    let transaction = GlobalRemovalTransaction {
        groups: std::slice::from_ref(&group),
        cleanup: &cleanup,
        affected_bin_names: &affected_bin_names,
    };

    let error = remove_global_install_entries::<Host>(&transaction)
        .expect_err("a directory cannot be removed as a bin file");

    assert!(error.to_string().contains("remove global bin"));
    assert!(global_bin_dir.join("blocked").is_dir());
    assert!(!global_bin_dir.join("stale").exists());
    assert!(hash_link.exists());
    assert!(group.info.install_dir.exists());
}

#[test]
fn rollback_failure_keeps_recovery_artifacts() {
    RENAME_FAILURE_HASH_CALLS.store(0, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["tool"]);
    fixture.seed_file_slot("tool", b"old tool\n", 0o750);

    let error = activate_global_install::<RenameRollbackFailure>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            test_link_bins::<RenameRollbackFailure>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::new(),
            )
        },
    )
    .expect_err("the injected rollback rename must fail");

    let backup_dirs = backup_dirs(&fixture.global_bin_dir);
    assert_eq!(backup_dirs.len(), 1);
    let message = error.to_string();
    let diagnostic: &(dyn miette::Diagnostic + Send + Sync) = error.as_ref();
    assert_eq!(
        miette::Diagnostic::code(diagnostic).map(|code| code.to_string()),
        Some("ERR_PNPM_GLOBAL_BIN_ROLLBACK_FAILED".to_string()),
    );
    assert!(message.contains(&backup_dirs[0].display().to_string()));
    assert!(message.contains(&fixture.fresh_install_dir.display().to_string()));
    assert!(format!("{error:?}").contains("injected hash swap failure"));
    assert!(format!("{error:?}").contains("injected backup rename failure"));
    let activation_source =
        std::error::Error::source(diagnostic).expect("activation error must be the source");
    assert!(format!("{activation_source:?}").contains("injected hash swap failure"));
    assert!(
        diagnostic_source_messages(diagnostic)
            .iter()
            .any(|message| message.contains("injected hash swap failure")),
    );
    assert!(fixture.fresh_install_dir.exists());
    assert!(backup_dirs[0].exists());
}

#[test]
fn fresh_cleanup_failure_reports_only_remaining_fresh_install() {
    let _guard = hash_failure_guard();
    HASH_FAILURE_CALLS.store(0, Ordering::SeqCst);
    let root = tempfile::tempdir().expect("create cleanup failure fixture");
    let global_bin_dir = root.path().join("bin");
    let fresh_install_path = root.path().join("fresh-install");
    let hash_link = root.path().join("hash-link");
    let package_dir = root.path().join("replacement");
    fs::create_dir_all(package_dir.join("bin")).expect("create package bin directory");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    fs::write(&fresh_install_path, b"not a directory\n").expect("write non-directory install");
    fs::write(package_dir.join("bin/tool.js"), b"#!/usr/bin/env node\n")
        .expect("write package bin source");
    let packages = vec![PackageBinSource::new(
        package_dir,
        Arc::new(json!({
            "name": "replacement",
            "version": "2.0.0",
            "bin": { "tool": "bin/tool.js" },
        })),
    )];

    let error = activate_global_install::<HashSwapFailure>(
        &fresh_install_path,
        &hash_link,
        &global_bin_dir,
        &packages,
        &HashSet::new(),
        || test_link_bins::<HashSwapFailure>(&packages, &global_bin_dir, &HashSet::new()),
    )
    .expect_err("removing a regular file as an install directory must fail cleanup");

    let diagnostic: &(dyn miette::Diagnostic + Send + Sync) = error.as_ref();
    assert!(miette::Diagnostic::code(diagnostic).is_none());
    assert!(backup_dirs(&global_bin_dir).is_empty());
    assert!(fresh_install_path.exists());
    let message = error.to_string();
    assert_eq!(
        message,
        format!(
            "Failed to clean up after global bin activation failed. Remaining artifacts: {}.",
            fresh_install_path.display(),
        ),
    );
    assert!(
        diagnostic_source_messages(diagnostic)
            .iter()
            .any(|message| message.contains("injected hash swap failure")),
    );
}

#[test]
fn backup_cleanup_failure_reports_only_remaining_backup_without_code() {
    let _guard = backup_cleanup_guard();
    BACKUP_CLEANUP_HASH_CALLS.store(0, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["tool"]);
    arm_backup_cleanup_blocker(&fixture.global_bin_dir);

    let error = activate_global_install::<BackupCleanupFailure>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            test_link_bins::<BackupCleanupFailure>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::new(),
            )
        },
    )
    .expect_err("the non-empty backup directory must fail cleanup");

    let diagnostic: &(dyn miette::Diagnostic + Send + Sync) = error.as_ref();
    assert!(miette::Diagnostic::code(diagnostic).is_none());
    let backup_dirs = backup_dirs(&fixture.global_bin_dir);
    assert_eq!(backup_dirs.len(), 1);
    assert!(backup_dirs[0].exists());
    assert!(!fixture.fresh_install_dir.exists());
    let message = error.to_string();
    assert_eq!(
        message,
        format!(
            "Failed to clean up after global bin activation failed. Remaining artifacts: {}.",
            backup_dirs[0].display(),
        ),
    );
    assert!(
        diagnostic_source_messages(diagnostic)
            .iter()
            .any(|message| message.contains("injected hash swap failure")),
    );
}

#[test]
fn artifact_probe_failure_is_related_and_not_reported_as_a_confirmed_path() {
    let _guard = backup_cleanup_guard();
    BACKUP_CLEANUP_HASH_CALLS.store(0, Ordering::SeqCst);
    ARTIFACT_PROBE_CALLS.store(0, Ordering::SeqCst);
    let fixture = ActivationFixture::new(&["tool"]);
    arm_backup_cleanup_blocker(&fixture.global_bin_dir);

    let error = activate_global_install::<ArtifactProbeFailure>(
        &fixture.fresh_install_dir,
        &fixture.hash_link,
        &fixture.global_bin_dir,
        &fixture.packages,
        &HashSet::new(),
        || {
            test_link_bins::<ArtifactProbeFailure>(
                &fixture.packages,
                &fixture.global_bin_dir,
                &HashSet::new(),
            )
        },
    )
    .expect_err("the artifact probe failure must be preserved");

    let diagnostic: &(dyn miette::Diagnostic + Send + Sync) = error.as_ref();
    let backup_dirs = backup_dirs(&fixture.global_bin_dir);
    assert_eq!(backup_dirs.len(), 1);
    assert!(backup_dirs[0].exists());
    assert!(!fixture.fresh_install_dir.exists());
    assert_eq!(error.to_string(), "Failed to clean up after global bin activation failed.");
    let related = miette::Diagnostic::related(diagnostic)
        .expect("cleanup failures must be related diagnostics")
        .collect::<Vec<_>>();
    assert_eq!(related.len(), 2);
    assert!(related[0].to_string().contains("remove global bin backup directory"));
    assert!(related[0].to_string().contains(&backup_dirs[0].display().to_string()));
    assert!(related[1].to_string().contains("inspect remaining rollback artifact"));
    assert!(related[1].to_string().contains(&backup_dirs[0].display().to_string()));
    assert!(related[1].to_string().contains("injected rollback artifact probe failure"));
}

#[test]
fn both_cleanup_failures_report_both_errors_and_remaining_artifacts() {
    let _guard = backup_cleanup_guard();
    BACKUP_CLEANUP_HASH_CALLS.store(0, Ordering::SeqCst);
    let root = tempfile::tempdir().expect("create aggregate cleanup fixture");
    let global_bin_dir = root.path().join("bin");
    let fresh_install_path = root.path().join("fresh-install");
    let hash_link = root.path().join("hash-link");
    let package_dir = root.path().join("replacement");
    fs::create_dir_all(package_dir.join("bin")).expect("create package bin directory");
    fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
    fs::write(&fresh_install_path, b"not a directory\n").expect("write non-directory install");
    fs::write(package_dir.join("bin/tool.js"), b"#!/usr/bin/env node\n")
        .expect("write package bin source");
    let packages = vec![PackageBinSource::new(
        package_dir,
        Arc::new(json!({
            "name": "replacement",
            "version": "2.0.0",
            "bin": { "tool": "bin/tool.js" },
        })),
    )];
    arm_backup_cleanup_blocker(&global_bin_dir);

    let error = activate_global_install::<BackupCleanupFailure>(
        &fresh_install_path,
        &hash_link,
        &global_bin_dir,
        &packages,
        &HashSet::new(),
        || test_link_bins::<BackupCleanupFailure>(&packages, &global_bin_dir, &HashSet::new()),
    )
    .expect_err("both cleanup operations must fail");

    let diagnostic: &(dyn miette::Diagnostic + Send + Sync) = error.as_ref();
    assert!(miette::Diagnostic::code(diagnostic).is_none());
    let backup_dirs = backup_dirs(&global_bin_dir);
    assert_eq!(backup_dirs.len(), 1);
    assert!(backup_dirs[0].exists());
    assert!(fresh_install_path.exists());
    let message = error.to_string();
    assert_eq!(
        message,
        format!(
            "Failed to clean up after global bin activation failed. Remaining artifacts: {}, {}.",
            backup_dirs[0].display(),
            fresh_install_path.display(),
        ),
    );
    let related = miette::Diagnostic::related(diagnostic)
        .expect("cleanup failures must be related diagnostics")
        .collect::<Vec<_>>();
    assert_eq!(related.len(), 2);
    let backup_error = std::error::Error::source(related[0]).expect("backup cleanup error source");
    assert!(related[0].to_string().contains("remove global bin backup directory"));
    assert!(related[0].to_string().contains(&backup_dirs[0].display().to_string()));
    assert!(related[0].to_string().contains(&backup_error.to_string()));
    let fresh_error = std::error::Error::source(related[1]).expect("fresh cleanup error source");
    assert!(related[1].to_string().contains("remove fresh global install directory"));
    assert!(related[1].to_string().contains(&fresh_install_path.display().to_string()));
    assert!(related[1].to_string().contains(&fresh_error.to_string()));
    assert!(
        diagnostic_source_messages(diagnostic)
            .iter()
            .any(|source| source.contains("injected hash swap failure")),
    );
}
