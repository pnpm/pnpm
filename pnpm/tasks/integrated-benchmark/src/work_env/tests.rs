use super::{
    BenchId, BenchmarkScenario, HyperfineCommand, PhaseEvent, WorkEnv, collect_pnpr_direct_ratios,
    create_install_script, non_trivial_cold_batch, pnpr_auth_config_key,
    pnpr_benchmark_config_yaml, read_phase_events, render_diagnostics_markdown,
    requires_fresh_pnpr_cold_batch_metrics, summarize_phase_events,
};
use std::{collections::HashMap, fs};

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

#[test]
fn pnpr_direct_ratios_pair_matching_revisions() {
    let commands = HashMap::from([
        (
            "pacquet@HEAD".to_string(),
            HyperfineCommand {
                command: "pacquet@HEAD".to_string(),
                command_name: None,
                mean: 10.0,
            },
        ),
        (
            "pnpr@HEAD".to_string(),
            HyperfineCommand { command: "pnpr@HEAD".to_string(), command_name: None, mean: 8.0 },
        ),
        (
            "pnpr@main".to_string(),
            HyperfineCommand { command: "pnpr@main".to_string(), command_name: None, mean: 9.0 },
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
fn diagnostics_markdown_includes_create_virtual_store_line_item() {
    let markdown = render_diagnostics_markdown(
        &super::BenchmarkDiagnostics {
            targets: vec![super::BenchmarkTargetDiagnostics {
                id: "pnpr@HEAD".to_string(),
                hyperfine_mean_seconds: Some(7.5),
                phase_summary: super::PhaseSummary {
                    partition: Some(super::PartitionMetric {
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
                phase_summary: super::PhaseSummary::default(),
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
                phase_summary: super::PhaseSummary {
                    partition: Some(super::PartitionMetric {
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
    let manifest_dir = current.join("pnpm").join("crates").join("cli");
    fs::create_dir_all(&manifest_dir).expect("create current-layout manifest dir");
    fs::write(
        manifest_dir.join("Cargo.toml"),
        "[package]\nname       = \"pacquet-cli\"\n\n[[bin]]\nname = \"pnpm\"\n",
    )
    .expect("write current-layout manifest");
    assert_eq!(WorkEnv::cli_bin_name(&current), "pnpm");

    // Old layout, `pacquet` bin.
    let old = root.join("old");
    let manifest_dir = old.join("pacquet").join("crates").join("cli");
    fs::create_dir_all(&manifest_dir).expect("create old-layout manifest dir");
    fs::write(
        manifest_dir.join("Cargo.toml"),
        "[package]\nname = \"pacquet-cli\"\n\n[[bin]]\nname = \"pacquet\"\n",
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
