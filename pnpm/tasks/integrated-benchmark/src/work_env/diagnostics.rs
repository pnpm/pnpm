use super::{
    BENCHMARK_DIAGNOSTICS_JSON, BENCHMARK_DIAGNOSTICS_MD, BENCHMARK_OUTPUT_LOG,
    BenchmarkDiagnostics, BenchmarkTargetDiagnostics, HyperfineCommand,
    PNPR_DIRECT_ABS_SLACK_SECONDS, PNPR_DIRECT_RATIO_MAX, WorkEnv, check_peer_heavy_speedup,
    collect_pnpr_direct_ratios, dir_contains_file, non_trivial_cold_batch,
    read_benchmark_diagnostics, read_hyperfine_report, read_phase_events,
    render_diagnostics_markdown, requires_fresh_pnpr_cold_batch_metrics, summarize_phase_events,
};
use crate::cli_args::{BenchmarkScenario, TargetKind};
use std::{collections::HashMap, fs};

impl WorkEnv {
    /// Fail the run if a `pnpr@<rev>` target never actually went through its
    /// pnpr server. A resolve populates the server's on-disk store/cache
    /// under `pnpr-storage`, so an empty `pnpr-storage` after the
    /// benchmark means the client resolved *directly* instead — the silent
    /// failure mode where a misnamed `PNPM_CONFIG_PNPR_SERVER` made every
    /// `pnpr@<rev>` row a duplicate of its `pacquet@<rev>` row. Better to
    /// abort than to publish meaningless pnpr-vs-direct numbers.
    pub(super) fn verify_pnpr_targets_were_routed(&self) {
        for id in self.target_ids().filter(|id| id.is_pnpr()) {
            let storage = self.bench_dir(id).join("pnpr-storage");
            assert!(
                dir_contains_file(&storage),
                "pnpr server storage at {storage:?} is empty after the benchmark — `{id}` never \
                 routed through pnpr (it resolved directly), so its rows would silently duplicate \
                 the `pacquet@<rev>` install. Check that `.pnpr-env` exports \
                 `PNPM_CONFIG_PNPR_SERVER` and that the client reads it.",
            );
        }
    }
    pub(super) fn write_benchmark_diagnostics(&self) {
        let diagnostics = self.collect_benchmark_diagnostics();
        let json = serde_json::to_string_pretty(&diagnostics).expect("serialize diagnostics JSON");
        fs::write(self.root().join(BENCHMARK_DIAGNOSTICS_JSON), json)
            .expect("write benchmark diagnostics JSON");
        let markdown = render_diagnostics_markdown(&diagnostics, self.scenario);
        fs::write(self.root().join(BENCHMARK_DIAGNOSTICS_MD), &markdown)
            .expect("write benchmark diagnostics markdown");
    }
    pub(super) fn collect_benchmark_diagnostics(&self) -> BenchmarkDiagnostics {
        let hyperfine = read_hyperfine_report(&self.root().join("BENCHMARK_REPORT.json"));
        let commands_by_name: HashMap<String, HyperfineCommand> = hyperfine
            .results
            .into_iter()
            .map(|command| (command.name().to_string(), command))
            .collect();
        let targets = self
            .benchmarked_ids()
            .map(|id| {
                let id = id.to_string();
                let phase_events =
                    read_phase_events(&self.root().join(&id).join(BENCHMARK_OUTPUT_LOG));
                let command = commands_by_name.get(&id);
                BenchmarkTargetDiagnostics {
                    id,
                    hyperfine_mean_seconds: command.map(|command| command.mean),
                    hyperfine_min_seconds: command.map(|command| command.min),
                    phase_summary: summarize_phase_events(&phase_events),
                    phase_events,
                }
            })
            .collect();

        BenchmarkDiagnostics {
            targets,
            pnpr_direct_ratios: collect_pnpr_direct_ratios(&commands_by_name),
        }
    }
    pub(super) fn verify_benchmark_diagnostics(&self) {
        let diagnostics = read_benchmark_diagnostics(&self.root().join(BENCHMARK_DIAGNOSTICS_JSON));
        self.verify_peer_heavy_lockfiles();
        self.verify_fresh_pnpr_cold_batch(&diagnostics);
        self.verify_pnpr_direct_ratios(&diagnostics);
        self.verify_peer_heavy_pacquet_pnpm_ratio(&diagnostics);
    }
    pub(super) fn verify_peer_heavy_lockfiles(&self) {
        if self.scenario != Some(BenchmarkScenario::IsolatedPeerHeavyResolveHotCacheOffline) {
            return;
        }
        let mut lockfiles = self.target_ids().map(|id| {
            let path = self.bench_dir(id).join("pnpm-lock.yaml");
            let contents =
                fs::read(&path).unwrap_or_else(|error| panic!("read {path:?} for {id}: {error}"));
            (id.to_string(), contents)
        });
        let (reference_id, reference) =
            lockfiles.next().expect("peer-heavy benchmark has at least one target");
        for (target_id, lockfile) in lockfiles {
            assert!(
                lockfile == reference,
                "peer-heavy lockfile from {target_id} differs from {reference_id}",
            );
        }
    }
    pub(super) fn verify_fresh_pnpr_cold_batch(&self, diagnostics: &BenchmarkDiagnostics) {
        if self.scenario != Some(BenchmarkScenario::IsolatedFreshInstallColdCacheColdStore) {
            return;
        }
        for target in diagnostics
            .targets
            .iter()
            .filter(|target| requires_fresh_pnpr_cold_batch_metrics(&target.id))
        {
            let Some(partition) = target.phase_summary.partition.as_ref() else {
                panic!(
                    "{id} did not emit create_virtual_store_partition metrics; \
                     benchmark cannot prove the pnpr fresh install exercised the cold batch",
                    id = target.id,
                );
            };
            assert!(
                non_trivial_cold_batch(partition.cold, partition.total),
                "{id} did not exercise a non-trivial cold batch: warm={} cold={} skipped={} total={}",
                partition.warm,
                partition.cold,
                partition.skipped,
                partition.total,
                id = target.id,
            );
        }
    }
    pub(super) fn verify_pnpr_direct_ratios(&self, diagnostics: &BenchmarkDiagnostics) {
        let Some(scenario) = self.scenario else { return };
        if !scenario.expects_pnpr_not_slower_than_direct() {
            return;
        }
        for ratio in &diagnostics.pnpr_direct_ratios {
            if ratio.revision != "HEAD" {
                continue;
            }
            assert!(
                ratio.ratio <= PNPR_DIRECT_RATIO_MAX
                    || ratio.pnpr_mean_seconds - ratio.pacquet_mean_seconds
                        <= PNPR_DIRECT_ABS_SLACK_SECONDS,
                "pnpr@{} was slower than pacquet@{}: ratio {:.3} > {:.3} (pnpr {:.3}s, pacquet {:.3}s)",
                ratio.revision,
                ratio.revision,
                ratio.ratio,
                PNPR_DIRECT_RATIO_MAX,
                ratio.pnpr_mean_seconds,
                ratio.pacquet_mean_seconds,
            );
        }
    }
    pub(super) fn verify_peer_heavy_pacquet_pnpm_ratio(&self, diagnostics: &BenchmarkDiagnostics) {
        if self.scenario != Some(BenchmarkScenario::IsolatedPeerHeavyResolveHotCacheOffline)
            || !self
                .targets
                .iter()
                .any(|target| target.kind == TargetKind::Pnpm && target.rev == "HEAD")
            || !self
                .targets
                .iter()
                .any(|target| target.kind == TargetKind::Pacquet && target.rev == "HEAD")
        {
            return;
        }
        if let Err(message) = check_peer_heavy_speedup(diagnostics) {
            panic!("{message}");
        }
    }
}
