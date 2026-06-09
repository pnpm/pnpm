use super::{
    BenchmarkScenario, HyperfineCommand, PhaseEvent, collect_pnpr_direct_ratios,
    non_trivial_cold_batch, read_phase_events, render_diagnostics_markdown,
    requires_fresh_pnpr_cold_batch_metrics, summarize_phase_events,
};
use std::{collections::HashMap, fs};

#[test]
fn phase_event_parser_reads_flat_and_nested_json_trace_fields() {
    let path = std::env::temp_dir()
        .join(format!("pacquet-integrated-benchmark-phase-events-{}.ndjson", std::process::id()));
    fs::write(
        &path,
        r#"{"target":"pacquet::install::phase","phase":"create_virtual_store_partition","warm":3,"cold":7,"skipped":1,"total":11}"#
            .to_string()
            + "\n"
            + r#"{"target":"pacquet::install::phase","fields":{"phase":"create_virtual_store","elapsed_ms":42}}"#
            + "\n"
            + r#"{"name":"pnpm:progress","status":"resolved"}"#
            + "\n",
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
