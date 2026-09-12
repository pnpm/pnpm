use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, bump_mtime, fs, install_with_peer_alias_deps,
    new_pacquet_command, read_lockfile,
};
use assert_cmd::assert::OutputAssertExt;

/// `@pnpm.e2e/abc-parent-with-missing-peers@1.0.0` depends on
/// `@pnpm.e2e/abc@1.0.0`, which declares `peer-a`, `peer-b`, and
/// `peer-c` as peer dependencies. The parent provides none of them.
/// With `auto-install-peers` enabled (pacquet's default, matching
/// pnpm), all three peers should appear in `node_modules/.pnpm/`.
/// Without the orchestrator's hoist loop they'd be missing, and the
/// peer-resolution issue list would carry three entries.
///
/// Transitive auto-installed peers are NOT also linked at
/// `node_modules/<alias>` — only the manifest's own dependencies become
/// importer-level lockfile entries, so transitive peers live in
/// `snapshots:` / `packages:` only and consumers reach them through
/// their parent's slot's `node_modules`. Hoisting them at the importer
/// would require listing them in `importer.dependencies`, which breaks
/// the lockfile/manifest satisfaction check and pushes every later
/// install onto the fresh-resolve path.
#[test]
fn auto_install_peers_hoists_missing_peers_at_importer() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/abc-parent-with-missing-peers": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    let entries: Vec<String> = fs::read_dir(&pnpm_dir)
        .map(|dir| {
            dir.filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    for peer in ["peer-a", "peer-b", "peer-c"] {
        // The registry's `^1.0.0` resolves to the latest 1.x; assert on
        // the slot prefix rather than a specific version so a registry
        // bump doesn't churn this test.
        let prefix = format!("@pnpm.e2e+{peer}@1.");
        assert!(
            entries.iter().any(|name| name.starts_with(&prefix) && !name.contains('_')),
            "expected {peer} to be auto-installed; .pnpm/ entries: {entries:?}",
        );
    }

    drop((root, mock_instance));
}

/// `peer-diamond-plugin` peer-depends both `peer-diamond-parser` and
/// `peer-diamond-ts`, and `peer-diamond-parser` peer-depends
/// `peer-diamond-ts`. The plugin's parser and its ts must agree: when
/// the plugin resolves `ts@1.0.0`, its parser peer must also be the
/// `ts@1.0.0` instance, not a `ts@2.0.0` parser hoisted at the root.
///
/// This is the scenario behind the pnpm regression in
/// [pnpm/pnpm#12079](https://github.com/pnpm/pnpm/issues/12079). pacquet
/// resolves it consistently by switching from the inherited same-version
/// parser to the node's own child when that inherited parser carries a
/// conflicting peer context.
#[test]
fn peer_shared_through_a_diamond_is_resolved_consistently() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/peer-diamond-ts": "2.0.0",
            "@pnpm.e2e/peer-diamond-parser": "1.0.0",
            "@pnpm.e2e/peer-diamond-app": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    let consistent = "@pnpm.e2e/peer-diamond-plugin@1.0.0(@pnpm.e2e/peer-diamond-parser@1.0.0(@pnpm.e2e/peer-diamond-ts@1.0.0))(@pnpm.e2e/peer-diamond-ts@1.0.0)";
    let inconsistent = "@pnpm.e2e/peer-diamond-plugin@1.0.0(@pnpm.e2e/peer-diamond-parser@1.0.0(@pnpm.e2e/peer-diamond-ts@2.0.0))";
    assert!(
        lockfile.contains(consistent),
        "expected the plugin to share ts@1.0.0 with its parser; lockfile:\n{lockfile}",
    );
    assert!(
        !lockfile.contains(inconsistent),
        "the plugin must not be paired with a ts@2.0.0 parser; lockfile:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn transitive_pending_peer_uses_provider_final_suffix_in_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/final-peer-a": "1.0.0",
            "@pnpm.e2e/final-peer-c": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    let expected = "@pnpm.e2e/final-peer-x@1.0.0(@pnpm.e2e/final-peer-b@1.0.0(@pnpm.e2e/final-peer-a@1.0.0(@pnpm.e2e/final-peer-c@1.0.0)))";
    let provisional =
        "@pnpm.e2e/final-peer-x@1.0.0(@pnpm.e2e/final-peer-b@1.0.0(@pnpm.e2e/final-peer-a@1.0.0))";

    assert!(
        lockfile.contains(expected),
        "transitive peer must use the provider's final peer suffix; lockfile:\n{lockfile}",
    );
    assert!(
        !lockfile.contains(provisional),
        "lockfile must not keep the provider's provisional peer suffix; lockfile:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn peer_dependencies_resolve_from_aliased_subdependencies() {
    let lockfile = install_with_peer_alias_deps(serde_json::json!({
        "@pnpm.e2e/abc-parent-with-aliases": "1.0.0",
    }));

    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@1.0.1)(@pnpm.e2e/peer-b@1.0.0)(@pnpm.e2e/peer-c@1.0.1)"),
        "aliased subdependencies should satisfy abc's peers; lockfile:\n{lockfile}",
    );
}

#[test]
fn peer_dependency_resolves_from_aliased_direct_dependency() {
    let lockfile = install_with_peer_alias_deps(serde_json::json!({
        "peer-a": "npm:@pnpm.e2e/peer-a@1.0.0",
        "@pnpm.e2e/abc": "1.0.0",
    }));

    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@1.0.0)"),
        "aliased direct dependency should satisfy abc's peer-a; lockfile:\n{lockfile}",
    );
}

#[test]
fn peer_dependency_resolves_from_alias_that_differs_from_real_name() {
    let lockfile = install_with_peer_alias_deps(serde_json::json!({
        "@pnpm.e2e/peer-b": "npm:@pnpm.e2e/peer-a@1.0.0",
        "@pnpm.e2e/abc": "1.0.0",
    }));

    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@1.0.0)(@pnpm.e2e/peer-a@1.0.0)"),
        "abc's snapshot key should keep both peer-a contributions; lockfile:\n{lockfile}",
    );
    assert!(
        lockfile.contains("'@pnpm.e2e/peer-a': 1.0.0"),
        "real peer name should be linked in abc's snapshot dependencies; lockfile:\n{lockfile}",
    );
    assert!(
        lockfile.contains("'@pnpm.e2e/peer-b': '@pnpm.e2e/peer-a@1.0.0'"),
        "alias peer name should also be linked to the aliased provider; lockfile:\n{lockfile}",
    );
}

#[test]
fn peer_dependency_prefers_highest_version_among_aliases_of_same_package() {
    let lockfile = install_with_peer_alias_deps(serde_json::json!({
        "peer-c3": "npm:@pnpm.e2e/peer-c@1.0.0",
        "peer-c2": "npm:@pnpm.e2e/peer-c@1.0.1",
        "peer-c1": "npm:@pnpm.e2e/peer-c@2.0.0",
        "@pnpm.e2e/abc": "1.0.0",
    }));

    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-c@2.0.0)"),
        "highest aliased peer-c version should satisfy abc's peer-c; lockfile:\n{lockfile}",
    );
}

#[test]
fn peer_dependency_prefers_non_aliased_provider_over_alias() {
    let lockfile = install_with_peer_alias_deps(serde_json::json!({
        "@pnpm.e2e/peer-c": "1.0.0",
        "peer-c": "npm:@pnpm.e2e/peer-c@2.0.0",
        "@pnpm.e2e/abc": "1.0.0",
    }));

    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-c@1.0.0)"),
        "non-aliased peer-c should win over the aliased provider; lockfile:\n{lockfile}",
    );
}

/// Adding a dependent to a manifest whose aliased peer providers are
/// already in the lockfile must bind the peer the same way a fresh
/// install of the full manifest does.
#[test]
fn peer_dependency_binds_the_same_when_added_to_an_existing_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("autoInstallPeers: false\n");
    workspace_yaml.push_str("strictPeerDependencies: false\n");
    workspace_yaml.push_str("peersSuffixMaxLength: 1000\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": {
            "peer-c3": "npm:@pnpm.e2e/peer-c@1.0.0",
            "peer-c2": "npm:@pnpm.e2e/peer-c@1.0.1",
            "peer-c1": "npm:@pnpm.e2e/peer-c@2.0.0",
        } })
        .to_string(),
    )
    .expect("write package.json");
    pacquet.with_arg("install").assert().success();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": {
            "peer-c3": "npm:@pnpm.e2e/peer-c@1.0.0",
            "peer-c2": "npm:@pnpm.e2e/peer-c@1.0.1",
            "peer-c1": "npm:@pnpm.e2e/peer-c@2.0.0",
            "@pnpm.e2e/abc": "1.0.0",
        } })
        .to_string(),
    )
    .expect("rewrite package.json");
    bump_mtime(&workspace.join("package.json"));
    new_pacquet_command(&workspace).with_arg("install").assert().success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-c@2.0.0)"),
        "re-resolving with a lockfile must bind the same provider as a fresh install; lockfile:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn peer_dependency_prefers_highest_aliased_subdependency_version() {
    let lockfile = install_with_peer_alias_deps(serde_json::json!({
        "@pnpm.e2e/abc-parent-with-aliases-of-same-pkg": "1.0.0",
    }));

    assert!(
        lockfile.contains("@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-c@2.0.0)"),
        "highest aliased peer-c subdependency should satisfy abc's peer-c; lockfile:\n{lockfile}",
    );
}

/// `resolutionMode: highest` (the default) resolves a direct dependency
/// to the highest version satisfying its range. `@pnpm.e2e/foo`
/// publishes `100.0.0` and `100.1.0`; `^100.0.0` therefore lands on
/// `100.1.0`.
#[test]
fn resolution_mode_highest_picks_highest_direct_version() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    assert!(
        pnpm_dir.join("@pnpm.e2e+foo@100.1.0").exists(),
        "highest mode must resolve ^100.0.0 to 100.1.0",
    );
    assert!(!pnpm_dir.join("@pnpm.e2e+foo@100.0.0").exists());

    drop((root, mock_instance));
}

/// `resolutionMode: lowest-direct` resolves a direct dependency to the
/// lowest version satisfying its range. With `@pnpm.e2e/foo` at
/// `100.0.0` / `100.1.0`, `^100.0.0` lands on `100.0.0` — the opposite
/// of the default. Proves the setting flows from `pnpm-workspace.yaml`
/// through the config layer into the resolver's version pick.
///
/// `minimumReleaseAge: 0` disables the maturity cutoff for this test:
/// while a cutoff is active the picker prefers the highest mature
/// version regardless of `resolutionMode`, so the lowest-version pick
/// would be masked.
#[test]
fn resolution_mode_lowest_direct_picks_lowest_direct_version() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing.push_str("resolutionMode: lowest-direct\nminimumReleaseAge: 0\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    assert!(
        pnpm_dir.join("@pnpm.e2e+foo@100.0.0").exists(),
        "lowest-direct mode must resolve ^100.0.0 to 100.0.0",
    );
    assert!(!pnpm_dir.join("@pnpm.e2e+foo@100.1.0").exists());

    drop((root, mock_instance));
}

/// `minimumReleaseAge` narrows the versions on offer to the mature ones;
/// it does not say which end of what is left to take, which is what
/// `resolutionMode` says. The pair has to keep working together, since
/// `minimumReleaseAge` is on by default.
#[test]
fn resolution_mode_lowest_direct_applies_under_a_minimum_release_age() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing.push_str("resolutionMode: lowest-direct\nminimumReleaseAge: 1\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    assert!(
        pnpm_dir.join("@pnpm.e2e+foo@100.0.0").exists(),
        "lowest-direct must still resolve ^100.0.0 to 100.0.0 when a release age is configured",
    );
    assert!(!pnpm_dir.join("@pnpm.e2e+foo@100.1.0").exists());

    drop((root, mock_instance));
}

/// `time-based` picks the lowest satisfying version for a direct
/// dependency too, so it has to survive a release-age cutoff the same way
/// `lowest-direct` does.
#[test]
fn resolution_mode_time_based_applies_under_a_minimum_release_age() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing.push_str("resolutionMode: time-based\nminimumReleaseAge: 1\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    assert!(
        pnpm_dir.join("@pnpm.e2e+foo@100.0.0").exists(),
        "time-based must still resolve ^100.0.0 to 100.0.0 when a release age is configured",
    );
    assert!(!pnpm_dir.join("@pnpm.e2e+foo@100.1.0").exists());

    drop((root, mock_instance));
}

/// A hoisted (auto-installed) peer is not a dependency the user
/// declared, so the direct-dep pick of `lowest-direct` must not apply
/// to it even though it installs at the importer level.
#[test]
fn resolution_mode_lowest_direct_resolves_hoisted_peers_to_highest() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing
        .push_str("resolutionMode: lowest-direct\nminimumReleaseAge: 0\nautoInstallPeers: true\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/abc-parent-with-missing-peers": "1.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    assert!(
        pnpm_dir.join("@pnpm.e2e+peer-a@1.0.1").exists(),
        "the hoisted peer must resolve to the highest satisfying version under lowest-direct",
    );
    assert!(!pnpm_dir.join("@pnpm.e2e+peer-a@1.0.0").exists());

    drop((root, mock_instance));
}

/// `time-based` shares the hoisted-peer rule with `lowest-direct`: the
/// hoist resolves like a transitive dep — highest satisfying, under the
/// subdep publish-date cutoff.
#[test]
fn resolution_mode_time_based_resolves_hoisted_peers_to_highest() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing.push_str("resolutionMode: time-based\nminimumReleaseAge: 0\nautoInstallPeers: true\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/abc-parent-with-missing-peers": "1.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let pnpm_dir = workspace.join("node_modules/.pnpm");
    assert!(
        pnpm_dir.join("@pnpm.e2e+peer-a@1.0.1").exists(),
        "the hoisted peer must resolve to the highest satisfying version under time-based",
    );
    assert!(!pnpm_dir.join("@pnpm.e2e+peer-a@1.0.0").exists());

    drop((root, mock_instance));
}

/// Dropping `time:` would lose the publish dates a re-resolve falls back
/// on when the registry's abbreviated metadata carries none, changing the
/// cutoff every subdependency is resolved under.
#[test]
fn time_based_install_records_and_preserves_the_lockfile_time_section() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing.push_str("resolutionMode: time-based\nminimumReleaseAge: 0\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let recorded =
        read_lockfile(&lockfile_path).time.expect("a time-based install records `time:`");
    assert_eq!(
        recorded.keys().collect::<Vec<_>>(),
        ["@pnpm.e2e/foo@100.0.0"],
        "only the direct dependency's publish date is recorded: {recorded:?}",
    );

    new_pacquet_command(&workspace).with_arg("install").assert().success();

    assert_eq!(read_lockfile(&lockfile_path).time.as_ref(), Some(&recorded));

    drop((root, mock_instance));
}

/// `@pnpm.e2e/abc-parent-with-ab@1.0.0` transitively peer-depends on
/// `@pnpm.e2e/peer-c` (through its `@pnpm.e2e/abc` dependency). A diamond
/// reaches it in two compatible peer contexts: the root supplies
/// `peer-c@2.0.0` directly, while `@pnpm.e2e/abc-grand-parent-with-c` supplies
/// its own `peer-c@^1.0.0`. The root's exact `abc-parent-with-ab@1.0.0` pin
/// seeds preferred versions so the grand-parent's `^1.0.0` resolves to the
/// same `1.0.0`, leaving two distinct peer-suffixed snapshots.
///
/// The first install records both. The second install adds a new dep — which
/// defeats the up-to-date short-circuit so the writable fresh-lockfile path
/// re-resolves the tree against the prior lockfile, reusing
/// `abc-parent-with-ab` in both contexts via the lockfile-reuse path. That
/// reuse must preserve both contexts instead of collapsing the two
/// occurrences onto one (bare) snapshot.
#[test]
fn compatible_existing_peer_contexts_survive_writable_lockfile_regeneration() {
    // The binary is re-spawned per install via `new_pacquet_command`, so the
    // `CommandTempCwd::pacquet` builder is not used here.
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // The root pins `abc-parent-with-ab@1.0.0` (the root's peer-c@2.0.0
    // context) and also pulls in `abc-grand-parent-with-c`, which depends on
    // `abc-parent-with-ab@^1.0.0` plus its own `peer-c@^1.0.0` (the nested
    // peer-c@1.x context). The root's exact `1.0.0` pin seeds preferred
    // versions so the grand-parent's `^1.0.0` resolves to the same `1.0.0`,
    // giving two compatible peer contexts of the same `abc-parent-with-ab`.
    let install_with = |deps: serde_json::Value| {
        fs::write(
            workspace.join("package.json"),
            serde_json::json!({ "dependencies": deps }).to_string(),
        )
        .expect("write package.json");
        new_pacquet_command(&workspace).with_arg("install").assert().success();
    };

    let root_context = "@pnpm.e2e/abc-parent-with-ab@1.0.0(@pnpm.e2e/peer-c@2.0.0)";
    let nested_context_prefix = "@pnpm.e2e/abc-parent-with-ab@1.0.0(@pnpm.e2e/peer-c@1.";

    eprintln!("First install: records both peer-c contexts...");
    install_with(serde_json::json!({
        "@pnpm.e2e/abc-grand-parent-with-c": "1.0.0",
        "@pnpm.e2e/peer-c": "2.0.0",
        "@pnpm.e2e/abc-parent-with-ab": "1.0.0",
    }));

    let first = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        first.contains(nested_context_prefix) && first.contains(root_context),
        "first install must record both peer-c contexts; lockfile:\n{first}",
    );

    // Add a genuinely new dep. This defeats the up-to-date short-circuit, so
    // the writable fresh-lockfile resolution path runs and re-resolves the
    // tree against the prior lockfile — `abc-parent-with-ab` is reused in both
    // peer contexts via the lockfile-reuse path while only the new dep
    // resolves fresh.
    eprintln!("Second install re-resolves with the lockfile and must keep both contexts...");
    install_with(serde_json::json!({
        "@pnpm.e2e/abc-grand-parent-with-c": "1.0.0",
        "@pnpm.e2e/peer-c": "2.0.0",
        "@pnpm.e2e/abc-parent-with-ab": "1.0.0",
        "@pnpm.e2e/foo": "100.0.0",
    }));

    let second = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        second.contains(nested_context_prefix),
        "reuse must preserve the nested peer-c@1.x context; lockfile:\n{second}",
    );
    assert!(
        second.contains(root_context),
        "reuse must preserve the root peer-c@2.0.0 context; lockfile:\n{second}",
    );

    drop((root, mock_instance));
}

/// `frozenLockfile: true` in `pnpm-workspace.yaml` drives the same
/// headless install `--frozen-lockfile` does, and `--no-frozen-lockfile`
/// overrides it back off.
#[test]
fn frozen_lockfile_accepts_a_peer_package_extensions_injected() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str(concat!(
        "autoInstallPeers: true\n",
        "packageExtensions:\n",
        "  root:\n",
        "    peerDependencies:\n",
        "      '@pnpm.e2e/foo': ^100.0.0\n",
    ));
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    // The extension made `@pnpm.e2e/foo` a peer of the project, so
    // `autoInstallPeers` recorded it as a dependency of the importer. The
    // freshness check has to see the same peer, or it reads that entry as a
    // dependency the manifest dropped.
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    assert!(
        wanted.importers["."].dependencies.as_ref().is_some_and(
            |dependencies| dependencies.contains_key(&"@pnpm.e2e/foo".parse().expect("alias"))
        ),
        "the injected peer is auto-installed into the importer",
    );

    new_pacquet_command(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    drop((root, mock_instance));
}
