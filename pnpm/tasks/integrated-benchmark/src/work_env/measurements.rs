use super::PACQUET_PNPM_SPEEDUP_MIN;
use crate::cli_args::BenchmarkScenario;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, fmt::Write as _, fs, path::Path};

/// How much faster pacquet resolved the peer-heavy DAG than the TypeScript CLI,
/// or the gate's failure message when that is under
/// [`PACQUET_PNPM_SPEEDUP_MIN`].
///
/// Both sides are each target's fastest run: see [`benchmark_target_min`].
pub(super) fn check_peer_heavy_speedup(diagnostics: &BenchmarkDiagnostics) -> Result<f64, String> {
    let pacquet = benchmark_target_min(diagnostics, "pacquet@HEAD");
    let pnpm = benchmark_target_min(diagnostics, "pnpm@HEAD");
    let speedup = pnpm / pacquet;
    if speedup < PACQUET_PNPM_SPEEDUP_MIN {
        return Err(format!(
            "pacquet@HEAD was only {speedup:.2}x faster than pnpm@HEAD on the peer-heavy DAG; \
             required at least {PACQUET_PNPM_SPEEDUP_MIN:.2}x \
             (pacquet {pacquet:.3}s, pnpm {pnpm:.3}s)",
        ));
    }
    Ok(speedup)
}
#[derive(Debug, Deserialize)]
pub(super) struct HyperfineReport {
    pub(super) results: Vec<HyperfineCommand>,
}
#[derive(Debug, Clone, Deserialize)]
pub(super) struct HyperfineCommand {
    pub(super) command: String,
    #[serde(default)]
    pub(super) command_name: Option<String>,
    pub(super) mean: f64,
    pub(super) min: f64,
}
impl HyperfineCommand {
    pub(super) fn name(&self) -> &str {
        self.command_name.as_deref().unwrap_or(&self.command)
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct BenchmarkDiagnostics {
    pub(super) targets: Vec<BenchmarkTargetDiagnostics>,
    pub(super) pnpr_direct_ratios: Vec<PnprDirectRatio>,
}
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct BenchmarkTargetDiagnostics {
    pub(super) id: String,
    pub(super) hyperfine_mean_seconds: Option<f64>,
    pub(super) hyperfine_min_seconds: Option<f64>,
    pub(super) phase_summary: PhaseSummary,
    pub(super) phase_events: Vec<PhaseEvent>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct PhaseSummary {
    pub(super) partition: Option<PartitionMetric>,
    pub(super) create_virtual_store_mean_ms: Option<f64>,
    pub(super) link_slots: Vec<LinkSlotsMetric>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PartitionMetric {
    pub(super) warm: u64,
    pub(super) cold: u64,
    pub(super) skipped: u64,
    pub(super) total: u64,
}
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct LinkSlotsMetric {
    pub(super) batch: String,
    slots: u64,
    pub(super) mean_ms: f64,
}
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct PhaseEvent {
    pub(super) phase: String,
    pub(super) elapsed_ms: Option<u64>,
    pub(super) warm: Option<u64>,
    pub(super) cold: Option<u64>,
    pub(super) skipped: Option<u64>,
    pub(super) total: Option<u64>,
    pub(super) batch: Option<String>,
    pub(super) slots: Option<u64>,
}
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct PnprDirectRatio {
    pub(super) revision: String,
    pub(super) pnpr_mean_seconds: f64,
    pub(super) pacquet_mean_seconds: f64,
    pub(super) ratio: f64,
}
pub(super) fn read_hyperfine_report(path: &Path) -> HyperfineReport {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read hyperfine report at {}: {err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("parse hyperfine report at {}: {err}", path.display()))
}
pub(super) fn read_benchmark_diagnostics(path: &Path) -> BenchmarkDiagnostics {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read benchmark diagnostics at {}: {err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("parse benchmark diagnostics at {}: {err}", path.display()))
}
pub(super) fn read_phase_events(path: &Path) -> Vec<PhaseEvent> {
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| {
            value.get("target").and_then(Value::as_str) == Some("pacquet::install::phase")
        })
        .filter_map(|value| {
            let phase = event_str(&value, "phase")?.to_string();
            Some(PhaseEvent {
                phase,
                elapsed_ms: event_u64(&value, "elapsed_ms"),
                warm: event_u64(&value, "warm"),
                cold: event_u64(&value, "cold"),
                skipped: event_u64(&value, "skipped"),
                total: event_u64(&value, "total"),
                batch: event_str(&value, "batch").map(str::to_string),
                slots: event_u64(&value, "slots"),
            })
        })
        .collect()
}
pub(super) fn event_field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).or_else(|| value.get("fields").and_then(|fields| fields.get(key)))
}
pub(super) fn event_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    event_field(value, key).and_then(Value::as_str)
}
pub(super) fn event_u64(value: &Value, key: &str) -> Option<u64> {
    let value = event_field(value, key)?;
    value.as_u64().or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}
pub(super) fn summarize_phase_events(events: &[PhaseEvent]) -> PhaseSummary {
    let partition =
        events.iter().rev().find(|event| event.phase == "create_virtual_store_partition").and_then(
            |event| {
                Some(PartitionMetric {
                    warm: event.warm?,
                    cold: event.cold?,
                    skipped: event.skipped.unwrap_or(0),
                    total: event.total?,
                })
            },
        );
    let create_virtual_store_mean_ms = mean(
        events
            .iter()
            .filter(|event| event.phase == "create_virtual_store")
            .filter_map(|event| event.elapsed_ms)
            .map(|elapsed| elapsed as f64),
    );
    let link_slots = ["warm", "cold"]
        .into_iter()
        .filter_map(|batch| {
            let matching: Vec<&PhaseEvent> = events
                .iter()
                .filter(|event| {
                    event.phase == "link_slots" && event.batch.as_deref() == Some(batch)
                })
                .collect();
            if matching.is_empty() {
                return None;
            }
            let slots = matching.iter().filter_map(|event| event.slots).max().unwrap_or(0);
            let mean_ms =
                mean(matching.iter().filter_map(|event| event.elapsed_ms).map(|ms| ms as f64))?;
            Some(LinkSlotsMetric { batch: batch.to_string(), slots, mean_ms })
        })
        .collect();
    PhaseSummary { partition, create_virtual_store_mean_ms, link_slots }
}
pub(super) fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut total = 0.0;
    let mut count = 0_u64;
    for value in values {
        total += value;
        count += 1;
    }
    (count > 0).then_some(total / count as f64)
}
pub(super) fn collect_pnpr_direct_ratios(
    commands_by_name: &HashMap<String, HyperfineCommand>,
) -> Vec<PnprDirectRatio> {
    let mut ratios: Vec<PnprDirectRatio> = commands_by_name
        .iter()
        .filter_map(|(name, pnpr)| {
            let revision = name.strip_prefix("pnpr@")?;
            let direct = commands_by_name.get(&format!("pacquet@{revision}"))?;
            Some(PnprDirectRatio {
                revision: revision.to_string(),
                pnpr_mean_seconds: pnpr.mean,
                pacquet_mean_seconds: direct.mean,
                ratio: pnpr.mean / direct.mean,
            })
        })
        .collect();
    ratios.sort_by(|a, b| a.revision.cmp(&b.revision));
    ratios
}
pub(super) fn non_trivial_cold_batch(cold: u64, total: u64) -> bool {
    cold > 0 && (total < 10 || cold.saturating_mul(10) >= total)
}
pub(super) fn requires_fresh_pnpr_cold_batch_metrics(target_id: &str) -> bool {
    target_id == "pnpr@HEAD"
}
/// The fastest of a target's timed runs.
///
/// Contention on a shared runner only ever *adds* time, so the minimum is the
/// sample least perturbed by a noisy neighbour and the most stable basis for a
/// cross-engine comparison. This is the same statistic the workflow reports to
/// Bencher, for the same reason.
pub(super) fn benchmark_target_min(diagnostics: &BenchmarkDiagnostics, target_id: &str) -> f64 {
    diagnostics
        .targets
        .iter()
        .find(|target| target.id == target_id)
        .and_then(|target| target.hyperfine_min_seconds)
        .unwrap_or_else(|| panic!("benchmark report has no min for required target {target_id}"))
}
pub(super) fn render_diagnostics_markdown(
    diagnostics: &BenchmarkDiagnostics,
    scenario: Option<BenchmarkScenario>,
) -> String {
    let mut out = String::from("## Pacquet benchmark diagnostics\n\n");
    if scenario == Some(BenchmarkScenario::IsolatedFreshInstallColdCacheColdStore)
        && contains_uninstrumented_pnpr_main(diagnostics)
    {
        out.push_str(
            "> Note: `pnpr@main` in this no-lockfile cold-store report predates the benchmark tarball URL rewrite, so newly resolved tarballs can use raw loopback registry URLs. `pnpr@HEAD` rewrites those URLs to the client-facing registry and pays the configured registry latency/bandwidth. Treat `pnpr@HEAD / pacquet@HEAD` as the guarded comparison here, not `pnpr@HEAD` versus `pnpr@main`.\n\n",
        );
    }
    out.push_str(
        "| Target | hyperfine mean | hyperfine min | warm | cold | skipped | CreateVirtualStore mean | link warm mean | link cold mean |\n",
    );
    out.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for target in &diagnostics.targets {
        let partition = target.phase_summary.partition.as_ref();
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            target.id,
            format_seconds(target.hyperfine_mean_seconds),
            format_seconds(target.hyperfine_min_seconds),
            format_u64(partition.map(|metric| metric.warm)),
            format_u64(partition.map(|metric| metric.cold)),
            format_u64(partition.map(|metric| metric.skipped)),
            format_ms(target.phase_summary.create_virtual_store_mean_ms),
            format_ms(link_slots_mean(&target.phase_summary, "warm")),
            format_ms(link_slots_mean(&target.phase_summary, "cold")),
        );
    }
    if !diagnostics.pnpr_direct_ratios.is_empty() {
        out.push_str("\n| Ratio | value |\n| --- | ---: |\n");
        for ratio in &diagnostics.pnpr_direct_ratios {
            let _ = writeln!(
                out,
                "| pnpr@{} / pacquet@{} | {:.3} |",
                ratio.revision, ratio.revision, ratio.ratio,
            );
        }
    }
    out
}
pub(super) fn contains_uninstrumented_pnpr_main(diagnostics: &BenchmarkDiagnostics) -> bool {
    diagnostics
        .targets
        .iter()
        .any(|target| target.id == "pnpr@main" && target.phase_summary.partition.is_none())
}
pub(super) fn link_slots_mean(summary: &PhaseSummary, batch: &str) -> Option<f64> {
    summary.link_slots.iter().find(|metric| metric.batch == batch).map(|metric| metric.mean_ms)
}
pub(super) fn format_seconds(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_string(), |value| format!("{value:.3}s"))
}
pub(super) fn format_ms(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_string(), |value| format!("{value:.1}ms"))
}
pub(super) fn format_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "-".to_string(), |value| value.to_string())
}
