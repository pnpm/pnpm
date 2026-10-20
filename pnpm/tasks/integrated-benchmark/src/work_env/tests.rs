use pnpr_storage::layout::local_document_path;

use super::{
    BenchId, HyperfineCommand, LINKED_WORKSPACE_DEPTH, LINKED_WORKSPACE_WIDTH, PEER_HEAVY_DEPTH,
    PEER_HEAVY_PROVIDER, PEER_HEAVY_WIDTH, WorkEnv, collect_pnpr_direct_ratios,
    create_generated_projects, create_install_script, create_package_json, create_pnpm_workspace,
    fixtures::peer_heavy_package_name,
    may_create_lockfile,
    measurements::PhaseEvent,
    non_trivial_cold_batch, read_phase_events, render_diagnostics_markdown,
    requires_fresh_pnpr_cold_batch_metrics, seed_peer_heavy_registry,
    server_config::{
        cold_mock_config_yaml, pnpr_auth_config_key, pnpr_benchmark_config_yaml,
        serves_npm_under_base_path,
    },
    summarize_phase_events,
};
use crate::cli_args::BenchmarkScenario;
use pnpm_lockfile::Lockfile;
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    thread,
};

#[test]
fn offline_scenario_writes_online_prewarm_script() {
    let dir = std::env::temp_dir()
        .join(format!("pacquet-integrated-benchmark-offline-prewarm-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create script test dir");

    create_install_script(
        &dir,
        BenchmarkScenario::IsolatedFreshResolveHotCacheOffline,
        "pnpm",
        BenchId::PnpmRevision("HEAD"),
    );
    let install = fs::read_to_string(dir.join("install.bash")).expect("read install.bash");
    let prewarm = fs::read_to_string(dir.join("prewarm.bash")).expect("read prewarm.bash");
    let _ = fs::remove_dir_all(&dir);

    assert!(install.contains("install --offline --lockfile-only"), "install = {install}");
    // The priming run must reach the registry: a plain online install.
    assert!(prewarm.ends_with("exec pnpm install\n"), "prewarm = {prewarm}");
}

#[test]
fn online_scenario_writes_no_prewarm_script() {
    let dir = std::env::temp_dir()
        .join(format!("pacquet-integrated-benchmark-online-prewarm-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create script test dir");

    create_install_script(
        &dir,
        BenchmarkScenario::IsolatedFreshRestoreHotCacheHotStore,
        "pnpm",
        BenchId::PnpmRevision("HEAD"),
    );
    let has_prewarm = dir.join("prewarm.bash").exists();
    let _ = fs::remove_dir_all(&dir);

    assert!(!has_prewarm);
}

/// The whys live on the `cleanup()` arm and the
/// `prewarms_node_modules` predicate this test pins.
#[test]
fn repeat_install_scenarios_keep_node_modules_populated() {
    for scenario in [
        BenchmarkScenario::IsolatedRepeatInstallHotCacheHotStore,
        BenchmarkScenario::IsolatedRepeatInstallColdCacheHotStore,
    ] {
        let cleanup = scenario.cleanup();
        assert!(
            !cleanup.remove.contains(&"node_modules"),
            "{scenario:?} must not wipe node_modules between iterations",
        );
        assert!(
            cleanup.restore.is_empty(),
            "{scenario:?} must not restore files a repeat install never mutates",
        );
        assert!(scenario.prewarms_node_modules(), "{scenario:?} must pre-warm node_modules");
        assert!(scenario.seeds_lockfile(), "{scenario:?} needs the seeded lockfile");
    }
}

#[test]
fn peer_heavy_scenario_generates_shared_subgraph_root() {
    let dir = std::env::temp_dir()
        .join(format!("pacquet-integrated-benchmark-peer-heavy-workspace-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create peer-heavy workspace test dir");

    create_package_json(&dir, None, BenchmarkScenario::IsolatedPeerHeavyResolveHotCacheOffline);
    create_pnpm_workspace(
        &dir,
        None,
        "http://localhost:4873/",
        BenchmarkScenario::IsolatedPeerHeavyResolveHotCacheOffline,
    );
    create_install_script(
        &dir,
        BenchmarkScenario::IsolatedPeerHeavyResolveHotCacheOffline,
        "pnpm",
        BenchId::PnpmRevision("HEAD"),
    );

    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(dir.join("package.json")).expect("read generated package manifest"),
    )
    .expect("parse generated package manifest");
    let workspace =
        fs::read_to_string(dir.join("pnpm-workspace.yaml")).expect("read generated workspace");
    let prewarm = fs::read_to_string(dir.join("prewarm.bash")).expect("read prewarm script");
    let registry = dir.join("registry");
    seed_peer_heavy_registry(&registry);
    let first_packument: serde_json::Value = serde_json::from_slice(
        &fs::read(local_document_path(&registry, "", &peer_heavy_package_name(0, 0)))
            .expect("read first peer-heavy packument"),
    )
    .expect("parse first peer-heavy packument");
    let leaf_packument: serde_json::Value = serde_json::from_slice(
        &fs::read(local_document_path(
            &registry,
            "",
            &peer_heavy_package_name(PEER_HEAVY_DEPTH - 1, 0),
        ))
        .expect("read leaf peer-heavy packument"),
    )
    .expect("parse leaf peer-heavy packument");
    let registry_package_count =
        fs::read_dir(registry.join("default/packages")).expect("read peer-heavy registry").count();
    let _ = fs::remove_dir_all(&dir);

    let dependencies = manifest["dependencies"].as_object().expect("generated dependencies");
    assert_eq!(dependencies.len(), PEER_HEAVY_WIDTH + 1);
    assert_eq!(dependencies[PEER_HEAVY_PROVIDER], "1.0.0");
    assert!(workspace.contains("packages:") && workspace.contains("- '.'"));
    assert!(prewarm.ends_with("exec pnpm install --lockfile-only\n"), "prewarm = {prewarm}");
    assert_eq!(registry_package_count, PEER_HEAVY_DEPTH * PEER_HEAVY_WIDTH + 1);
    assert_eq!(
        first_packument["versions"]["1.0.0"]["dependencies"]
            .as_object()
            .expect("first-layer dependencies")
            .len(),
        PEER_HEAVY_WIDTH,
    );
    assert_eq!(
        first_packument["versions"]["1.0.0"]["peerDependencies"][PEER_HEAVY_PROVIDER],
        "1.0.0",
    );
    assert!(
        leaf_packument["versions"]["1.0.0"]["dependencies"]
            .as_object()
            .expect("leaf dependencies")
            .is_empty(),
    );
}

/// The fixture's point is the `workspace:*` fan-out and the peer every
/// linked project declares: without both, the install never walks a shared
/// `link:` graph and the scenario stops guarding anything.
#[test]
fn linked_workspace_scenario_generates_a_shared_link_graph() {
    let scenario = BenchmarkScenario::IsolatedLinkedWorkspaceResolveHotCacheOffline;
    let dir = std::env::temp_dir()
        .join(format!("pacquet-integrated-benchmark-linked-workspace-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create linked-workspace test dir");

    create_package_json(&dir, None, scenario);
    super::linked_workspace::create_projects(&dir);
    create_pnpm_workspace(&dir, None, "http://localhost:4873/", scenario);

    let root: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("package.json")).expect("read root manifest"))
            .expect("parse root manifest");
    let first: serde_json::Value = serde_json::from_slice(
        &fs::read(dir.join("packages/level-0-00/package.json")).expect("read first project"),
    )
    .expect("parse first project");
    let leaf: serde_json::Value = serde_json::from_slice(
        &fs::read(dir.join(format!(
            "packages/level-{}-00/package.json",
            LINKED_WORKSPACE_DEPTH - 1,
        )))
        .expect("read leaf project"),
    )
    .expect("parse leaf project");
    let workspace =
        fs::read_to_string(dir.join("pnpm-workspace.yaml")).expect("read generated workspace");
    let project_count = fs::read_dir(dir.join("packages")).expect("read projects").count();
    let _ = fs::remove_dir_all(&dir);

    assert_eq!(project_count, LINKED_WORKSPACE_DEPTH * LINKED_WORKSPACE_WIDTH + 1);
    assert!(workspace.contains("- packages/*"), "workspace = {workspace}");
    dbg!(&root, &first, &leaf);
    assert_eq!(
        root["dependencies"]
            .as_object()
            .expect("root dependencies")
            .len(),
        LINKED_WORKSPACE_WIDTH + 1,
    );
    // Every project links the whole next level plus the shared peer provider.
    assert_eq!(
        first["dependencies"]
            .as_object()
            .expect("first dependencies")
            .len(),
        LINKED_WORKSPACE_WIDTH + 1,
    );
    assert_eq!(first["dependencies"]["@pnpmtest/linked-benchmark-level-1-00"], "workspace:*");
    assert_eq!(first["peerDependencies"]["@pnpmtest/linked-benchmark-shared"], "1.0.0");
    // The last level links nothing but the provider, so the graph terminates.
    assert_eq!(
        leaf["dependencies"]
            .as_object()
            .expect("leaf dependencies")
            .len(),
        1,
    );
}

#[test]
fn phase_event_parser_reads_flat_and_nested_json_trace_fields() {
    let path = std::env::temp_dir()
        .join(format!("pacquet-integrated-benchmark-phase-events-{}.ndjson", std::process::id()));
    fs::write(
        &path,
        concat!(
            r#"{"target":"pacquet::install::phase","phase":"create_virtual_store_partition","warm":3,"cold":7,"skipped":1,"total":11}"#,
            "\n",
            r#"{"target":"pacquet::install::phase","fields":{"phase":"create_virtual_store","elapsed_ms":42}}"#,
            "\n",
            r#"{"name":"pnpm:progress","status":"resolved"}"#,
            "\n",
        ),
    )
    .expect("write phase fixture");

    let events = read_phase_events(&path);
    let _ = fs::remove_file(path);

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].phase, "create_virtual_store_partition");
    assert_eq!(events[0].warm, Some(3));
    assert_eq!(events[0].cold, Some(7));
    assert_eq!(events[1].phase, "create_virtual_store");
    assert_eq!(events[1].elapsed_ms, Some(42));
}

#[test]
fn phase_summary_reports_partition_and_means() {
    let events = vec![
        PhaseEvent {
            phase: "create_virtual_store_partition".to_string(),
            elapsed_ms: None,
            warm: Some(1),
            cold: Some(9),
            skipped: Some(0),
            total: Some(10),
            batch: None,
            slots: None,
        },
        PhaseEvent {
            phase: "create_virtual_store".to_string(),
            elapsed_ms: Some(100),
            warm: None,
            cold: None,
            skipped: None,
            total: None,
            batch: None,
            slots: None,
        },
        PhaseEvent {
            phase: "create_virtual_store".to_string(),
            elapsed_ms: Some(200),
            warm: None,
            cold: None,
            skipped: None,
            total: None,
            batch: None,
            slots: None,
        },
        PhaseEvent {
            phase: "link_slots".to_string(),
            elapsed_ms: Some(30),
            warm: None,
            cold: None,
            skipped: None,
            total: None,
            batch: Some("cold".to_string()),
            slots: Some(9),
        },
    ];

    let summary = summarize_phase_events(&events);
    let partition = summary.partition.expect("partition summary");
    assert_eq!(partition.warm, 1);
    assert_eq!(partition.cold, 9);
    assert_eq!(summary.create_virtual_store_mean_ms, Some(150.0));
    assert_eq!(summary.link_slots[0].batch, "cold");
    #[expect(clippy::float_cmp, reason = "deterministic mean of fixed fixture inputs is exact")]
    {
        assert_eq!(summary.link_slots[0].mean_ms, 30.0);
    }
}

fn peer_heavy_diagnostics(pacquet: (f64, f64), pnpm: (f64, f64)) -> super::BenchmarkDiagnostics {
    let target = |id: &str, (mean, min): (f64, f64)| super::BenchmarkTargetDiagnostics {
        id: id.to_string(),
        hyperfine_mean_seconds: Some(mean),
        hyperfine_min_seconds: Some(min),
        phase_summary: super::measurements::PhaseSummary::default(),
        phase_events: vec![],
    };
    super::BenchmarkDiagnostics {
        targets: vec![target("pacquet@HEAD", pacquet), target("pnpm@HEAD", pnpm)],
        pnpr_direct_ratios: vec![],
    }
}

/// On a shared runner one contended sample drags a target's mean well past its
/// uncontended runs, so the gate reads each engine's fastest run instead. These
/// timings clear the floor on the minimum and fall under it on the mean.
#[test]
fn peer_heavy_gate_reads_the_fastest_run_not_the_mean() {
    let diagnostics = peer_heavy_diagnostics((2.5, 1.5), (2.9, 2.8));

    let mean_speedup = 2.9 / 2.5;
    assert!(
        mean_speedup < super::PACQUET_PNPM_SPEEDUP_MIN,
        "fixture must fail on the mean: {mean_speedup} >= {}",
        super::PACQUET_PNPM_SPEEDUP_MIN,
    );

    let verdict = super::check_peer_heavy_speedup(&diagnostics);
    dbg!(&verdict);
    assert!(verdict.is_ok(), "gate must clear the floor on the fastest runs");
}

/// A genuine blowup lands below the floor on every sample, so the gate must
/// still reject it.
#[test]
fn peer_heavy_gate_rejects_a_regression_on_every_sample() {
    let diagnostics = peer_heavy_diagnostics((3.1, 3.0), (2.9, 2.8));

    let message = super::check_peer_heavy_speedup(&diagnostics)
        .expect_err("a sub-floor speedup must fail the gate");
    assert!(message.contains("faster than pnpm@HEAD"), "{message}");
}

#[test]
fn pnpr_direct_ratios_pair_matching_revisions() {
    let commands = HashMap::from([
        (
            "pacquet@HEAD".to_string(),
            HyperfineCommand {
                command: "pacquet@HEAD".to_string(),
                command_name: None,
                mean: 10.0,
                min: 10.0,
            },
        ),
        (
            "pnpr@HEAD".to_string(),
            HyperfineCommand {
                command: "pnpr@HEAD".to_string(),
                command_name: None,
                mean: 8.0,
                min: 8.0,
            },
        ),
        (
            "pnpr@main".to_string(),
            HyperfineCommand {
                command: "pnpr@main".to_string(),
                command_name: None,
                mean: 9.0,
                min: 9.0,
            },
        ),
    ]);

    let ratios = collect_pnpr_direct_ratios(&commands);

    assert_eq!(ratios.len(), 1);
    assert_eq!(ratios[0].revision, "HEAD");
    #[expect(clippy::float_cmp, reason = "deterministic ratio of fixed fixture inputs is exact")]
    {
        assert_eq!(ratios[0].ratio, 0.8);
    }
}

#[test]
fn cold_batch_canary_requires_non_trivial_cold_share() {
    assert!(non_trivial_cold_batch(1, 1));
    assert!(non_trivial_cold_batch(10, 100));
    assert!(!non_trivial_cold_batch(0, 100));
    assert!(!non_trivial_cold_batch(9, 100));
}

#[test]
fn cold_batch_metrics_canary_targets_current_pnpr_revision() {
    assert!(requires_fresh_pnpr_cold_batch_metrics("pnpr@HEAD"));
    assert!(!requires_fresh_pnpr_cold_batch_metrics("pnpr@main"));
    assert!(!requires_fresh_pnpr_cold_batch_metrics("pacquet@HEAD"));
}

#[test]
fn pnpr_auth_config_key_uses_npmrc_nerf_shape() {
    assert_eq!(pnpr_auth_config_key("http://127.0.0.1:42509"), "//127.0.0.1:42509/");
    assert_eq!(pnpr_auth_config_key("http://localhost:4873/pnpr/"), "//localhost:4873/pnpr/");
}

#[test]
fn pnpr_benchmark_config_declares_local_registry_public() {
    let storage = std::env::temp_dir().join("pnpr-benchmark-config-test-storage");

    let yaml = pnpr_benchmark_config_yaml(
        &storage,
        &["http://localhost:4873/", "http://127.0.0.1:61824/"],
    );

    assert!(yaml.contains("registry: http://localhost:4873/"));
    assert!(yaml.contains("registry: http://127.0.0.1:61824/"));
    assert!(yaml.contains("allowedPrivateNetworks:"));
    assert!(yaml.contains("max_users: -1"));
    assert!(yaml.contains("htpasswd"));
}

#[test]
fn pnpr_benchmark_config_relies_on_the_builtin_npm_route() {
    let storage = std::env::temp_dir().join("pnpr-benchmark-config-test-storage");

    let yaml = pnpr_benchmark_config_yaml(&storage, &[]);

    // No operator-declared public routes: npmjs resolution comes from the
    // built-in route, so the config never spells out an npmjs registry rule.
    assert!(yaml.contains("public: []"));
    assert!(!yaml.contains("registry: https://registry.npmjs.org/"));
}

#[test]
fn cold_mock_config_declares_public_upstream_under_registries() {
    let storage = std::env::temp_dir().join("cold-mock-config-test-storage");

    let yaml = cold_mock_config_yaml(&storage, "http://127.0.0.1:4873");

    assert!(yaml.contains("registries:"));
    assert!(yaml.contains("type: upstream"));
    assert!(yaml.contains("defaultRegistry: npmjs"));
    assert!(!yaml.contains("mounts:"));
    assert!(!yaml.contains("defaultTarget:"));
}

#[test]
fn diagnostics_markdown_includes_create_virtual_store_line_item() {
    let markdown = render_diagnostics_markdown(
        &super::BenchmarkDiagnostics {
            targets: vec![super::BenchmarkTargetDiagnostics {
                id: "pnpr@HEAD".to_string(),
                hyperfine_mean_seconds: Some(7.5),
                hyperfine_min_seconds: Some(7.5),
                phase_summary: super::measurements::PhaseSummary {
                    partition: Some(super::measurements::PartitionMetric {
                        warm: 12,
                        cold: 88,
                        skipped: 0,
                        total: 100,
                    }),
                    create_virtual_store_mean_ms: Some(1234.0),
                    link_slots: vec![],
                },
                phase_events: vec![],
            }],
            pnpr_direct_ratios: vec![],
        },
        None,
    );

    assert!(markdown.contains("CreateVirtualStore mean"));
    assert!(markdown.contains("1234.0ms"));
    assert!(markdown.contains("| pnpr@HEAD |"));
}

#[test]
fn diagnostics_markdown_notes_fresh_install_cold_store_tarball_baseline_shift() {
    let markdown = render_diagnostics_markdown(
        &super::BenchmarkDiagnostics {
            targets: vec![super::BenchmarkTargetDiagnostics {
                id: "pnpr@main".to_string(),
                hyperfine_mean_seconds: Some(1.0),
                hyperfine_min_seconds: Some(1.0),
                phase_summary: super::measurements::PhaseSummary::default(),
                phase_events: vec![],
            }],
            pnpr_direct_ratios: vec![],
        },
        Some(BenchmarkScenario::IsolatedFreshInstallColdCacheColdStore),
    );

    assert!(markdown.contains("pnpr@main"));
    assert!(markdown.contains("tarball URL rewrite"));
    assert!(markdown.contains("pnpr@HEAD / pacquet@HEAD"));
}

#[test]
fn diagnostics_markdown_omits_baseline_note_after_pnpr_main_is_instrumented() {
    let markdown = render_diagnostics_markdown(
        &super::BenchmarkDiagnostics {
            targets: vec![super::BenchmarkTargetDiagnostics {
                id: "pnpr@main".to_string(),
                hyperfine_mean_seconds: Some(1.0),
                hyperfine_min_seconds: Some(1.0),
                phase_summary: super::measurements::PhaseSummary {
                    partition: Some(super::measurements::PartitionMetric {
                        warm: 0,
                        cold: 1,
                        skipped: 0,
                        total: 1,
                    }),
                    create_virtual_store_mean_ms: None,
                    link_slots: vec![],
                },
                phase_events: vec![],
            }],
            pnpr_direct_ratios: vec![],
        },
        Some(BenchmarkScenario::IsolatedFreshInstallColdCacheColdStore),
    );

    assert!(!markdown.contains("tarball URL rewrite"));
}

#[test]
fn cli_bin_name_reads_the_declared_bin_from_either_layout() {
    let root = std::env::temp_dir()
        .join(format!("pacquet-integrated-benchmark-cli-bin-name-{}", std::process::id()));

    // Current layout, `pnpm` bin, with taplo-style key padding.
    let current = root.join("current");
    let manifest_dir = current
        .join("pnpm")
        .join("crates")
        .join("cli");
    fs::create_dir_all(&manifest_dir).expect("create current-layout manifest dir");
    fs::write(
        manifest_dir.join("Cargo.toml"),
        "[package]\nname       = \"pnpm-cli\"\n\n[[bin]]\nname = \"pnpm\"\n",
    )
    .expect("write current-layout manifest");
    assert_eq!(WorkEnv::cli_bin_name(&current), "pnpm");

    // Old layout, `pacquet` bin.
    let old = root.join("old");
    let manifest_dir = old
        .join("pacquet")
        .join("crates")
        .join("cli");
    fs::create_dir_all(&manifest_dir).expect("create old-layout manifest dir");
    fs::write(
        manifest_dir.join("Cargo.toml"),
        "[package]\nname = \"pnpm-cli\"\n\n[[bin]]\nname = \"pacquet\"\n",
    )
    .expect("write old-layout manifest");
    assert_eq!(WorkEnv::cli_bin_name(&old), "pacquet");

    // No manifest at all: default to the current name.
    assert_eq!(WorkEnv::cli_bin_name(&root.join("missing")), "pnpm");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn client_binary_in_prefers_the_existing_binary() {
    let root = std::env::temp_dir()
        .join(format!("pacquet-integrated-benchmark-client-binary-{}", std::process::id()));
    let release = root.join("target").join("release");
    fs::create_dir_all(&release).expect("create release dir");

    // Nothing built yet: default to the `pnpm` path.
    assert_eq!(WorkEnv::client_binary_in(&root), release.join("pnpm"));

    // Only the old name exists (an older revision's build).
    fs::write(release.join("pacquet"), "old").expect("write pacquet binary");
    assert_eq!(WorkEnv::client_binary_in(&root), release.join("pacquet"));

    // Both exist: the current name wins.
    fs::write(release.join("pnpm"), "new").expect("write pnpm binary");
    assert_eq!(WorkEnv::client_binary_in(&root), release.join("pnpm"));

    let _ = fs::remove_dir_all(&root);
}

/// The scenario's contract: the seeded lockfile is out of date in exactly
/// one way (the direct dependencies with peers are missing from the
/// importer, with the package graph intact), the install links nothing and
/// trusts the lockfile, and the next iteration finds the lockfile out of
/// date again with no metadata cached.
#[test]
fn store_only_scenario_unlocks_the_peered_direct_deps_and_links_nothing() {
    let scenario = BenchmarkScenario::StoreOnlyUnlockedDepsColdCacheHotStore;
    let temp = tempfile::TempDir::new().expect("create store-only test dir");
    let dir = temp.path();

    create_package_json(dir, None, scenario);
    create_pnpm_workspace(dir, None, "http://localhost:4873/", scenario);
    create_install_script(dir, scenario, "pnpm", BenchId::PnpmRevision("HEAD"));
    may_create_lockfile(dir, scenario, None);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("package.json")).expect("read package.json"))
            .expect("parse package.json");
    let workspace = fs::read_to_string(dir.join("pnpm-workspace.yaml")).expect("read workspace");
    let install = fs::read_to_string(dir.join("install.bash")).expect("read install.bash");
    let seeded = Lockfile::load_wanted_from_dir(dir)
        .expect("read the seeded lockfile")
        .expect("a lockfile was seeded");
    let fixture = Lockfile::parse(crate::fixtures::LOCKFILE, Path::new("pnpm-lock.yaml"))
        .expect("parse the fixture lockfile")
        .expect("the fixture lockfile holds a document");

    let declared: usize = ["dependencies", "devDependencies", "optionalDependencies"]
        .iter()
        .map(|group| {
            manifest[group]
                .as_object()
                .map(serde_json::Map::len)
                .unwrap_or(0)
        })
        .sum();
    // The importer's direct dependencies, split into those whose locked
    // version carries a peer suffix and the rest.
    let locked = |lockfile: &Lockfile| -> (usize, usize) {
        let importer = &lockfile.importers["."];
        [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
            .into_iter()
            .flatten()
            .flat_map(HashMap::values)
            .fold((0, 0), |(peered, plain), spec| {
                if super::unlocked_deps::has_peer_suffix(spec) {
                    (peered + 1, plain)
                } else {
                    (peered, plain + 1)
                }
            })
    };
    let packages = |lockfile: &Lockfile| lockfile.packages.as_ref().map(HashMap::len);
    let (peered, plain) = locked(&fixture);
    assert_eq!(peered + plain, declared, "the fixture lockfile is current");
    assert!(peered > 0 && plain > 0, "the fixture has direct deps with and without peers");
    assert_eq!(locked(&seeded), (0, plain), "exactly the peered direct deps are unlocked");
    assert_eq!(packages(&seeded), packages(&fixture), "the package graph is kept whole");
    assert!(workspace.contains("enableModulesDir: false"), "workspace = {workspace}");
    assert!(workspace.contains("storeIndexFormat: files"), "workspace = {workspace}");
    assert!(workspace.contains("trustLockfile: true"), "workspace = {workspace}");
    assert!(install.ends_with("exec pnpm install --no-frozen-lockfile\n"), "install = {install}");
    let cleanup = scenario.cleanup();
    assert_eq!(cleanup.remove, ["cache-dir"], "the metadata cache is cold, the store is kept");
    assert_eq!(cleanup.restore, [("pnpm-lock.yaml", ".saved-pnpm-lock.yaml")]);
}

#[test]
fn injected_repeat_install_creates_its_lockfile_before_the_frozen_runs() {
    let scenario = BenchmarkScenario::IsolatedInjectedWorkspaceRepeatInstallHotCacheHotStore;
    let dir = std::env::temp_dir().join(format!("pnpm-benchmark-injected-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    create_package_json(&dir, None, scenario);
    create_pnpm_workspace(&dir, None, "http://localhost:4873/", scenario);
    create_install_script(&dir, scenario, "pnpm", BenchId::PnpmRevision("HEAD"));
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("package.json")).unwrap()).unwrap();
    assert_eq!(manifest["dependencies"]["injected-benchmark-library"], "workspace:*");
    assert_eq!(manifest["dependenciesMeta"]["injected-benchmark-library"]["injected"], true);
    let workspace = fs::read_to_string(dir.join("pnpm-workspace.yaml")).unwrap();
    assert!(workspace.contains("dedupeInjectedDeps: false"));
    assert!(workspace.contains("packages/*"));
    assert!(
        fs::read_to_string(dir.join("prewarm.bash"))
            .unwrap()
            .ends_with("exec pnpm install --offline --no-frozen-lockfile\n"),
    );
    assert!(
        fs::read_to_string(dir.join("install.bash")).unwrap().contains("install --frozen-lockfile"),
    );
    assert!(!scenario.seeds_lockfile());
    assert!(!scenario.prewarms_node_modules());
    assert!(scenario.cleanup().remove.is_empty());
    assert!(scenario.cleanup().restore.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cargo_repeat_install_seeds_workspace_and_store_before_frozen_runs() {
    let scenario = BenchmarkScenario::CargoRepeatInstallHotCacheHotStore;
    let temp = tempfile::TempDir::new().expect("create cargo benchmark test dir");
    let dir = temp.path();

    create_package_json(dir, None, scenario);
    create_generated_projects(dir, scenario);
    create_pnpm_workspace(dir, None, "http://localhost:4873/", scenario);
    create_install_script(dir, scenario, "pnpm", BenchId::PnpmRevision("HEAD"));
    super::cargo_workspace::seed_store(dir);

    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("package.json")).unwrap()).unwrap();
    assert_eq!(manifest["name"], "cargo-benchmark-root");
    let cargo_toml = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("\n[workspace]\n"), "cargo_toml = {cargo_toml}");
    assert!(dir.join("src/lib.rs").is_file());
    let cargo_lock = fs::read_to_string(dir.join("Cargo.lock")).unwrap();
    assert!(cargo_lock.contains("benchmark-crate-0000"));
    assert!(cargo_lock.contains("dependencies = [\n \"benchmark-crate-0001\",\n]"));
    assert!(cargo_lock.contains("dependencies = [\n \"benchmark-crate-0799\",\n]"));
    let workspace = fs::read_to_string(dir.join("pnpm-workspace.yaml")).unwrap();
    assert!(workspace.contains("cargo:\n  enabled: true"), "workspace = {workspace}");
    assert!(
        fs::read_to_string(dir.join("prewarm.bash"))
            .unwrap()
            .ends_with("exec pnpm install --offline --no-frozen-lockfile\n"),
    );
    assert!(
        fs::read_to_string(dir.join("install.bash")).unwrap().contains("install --frozen-lockfile"),
    );
    let store_dir = pnpm_store_dir::StoreDir::new(dir.join("store-dir"));
    let store_index = pnpm_store_dir::StoreIndex::open_readonly_in(&store_dir).unwrap();
    assert_eq!(store_index.keys().unwrap().len(), super::cargo_workspace::CRATE_COUNT);
    assert!(!scenario.seeds_lockfile());
    assert!(!scenario.prewarms_node_modules());
    assert!(scenario.cleanup().remove.is_empty());
    assert!(scenario.cleanup().restore.is_empty());
}

/// Answer one request on a loopback port with `status_line` and `body`.
fn serve_registry_directory_once(status_line: &'static str, body: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a registry directory stub");
    let port = listener
        .local_addr()
        .expect("stub address")
        .port();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept the directory request");
        let mut request = [0; 1024];
        let _ = stream.read(&mut request).expect("read the directory request");
        write!(
            stream,
            "{status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len(),
        )
        .expect("answer the directory request");
    });
    port
}

#[test]
fn registry_directory_reports_where_npm_is_served() {
    let prefixed = serve_registry_directory_once(
        "HTTP/1.1 200 OK",
        r#"{"ecosystems":{"npm":{"available":true,"prefixed":true}}}"#,
    );
    assert!(serves_npm_under_base_path(prefixed));

    let root = serve_registry_directory_once(
        "HTTP/1.1 200 OK",
        r#"{"ecosystems":{"npm":{"available":true,"prefixed":false}}}"#,
    );
    assert!(!serves_npm_under_base_path(root));

    let without_directory = serve_registry_directory_once("HTTP/1.1 404 Not Found", "{}");
    assert!(!serves_npm_under_base_path(without_directory));
}
