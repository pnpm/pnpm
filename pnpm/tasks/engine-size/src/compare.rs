use crate::cli_args::Lto;
use serde::Deserialize;

#[derive(Debug, PartialEq)]
pub struct SizeChange {
    pub lto: Lto,
    pub baseline_bytes: u64,
    pub current_bytes: u64,
    pub change_bytes: i128,
    pub change_percent: f64,
}

#[derive(Deserialize)]
struct SizeReport {
    target: String,
    #[serde(default = "unknown_probe_features")]
    probe_features: String,
    measurements: Vec<SizeMeasurement>,
}

fn unknown_probe_features() -> String {
    "unknown".to_string()
}

#[derive(Deserialize)]
struct SizeMeasurement {
    lto: Lto,
    stripped_size: u64,
}

pub fn compare_sizes(
    current: &[(Lto, u64)],
    baseline: &[(Lto, u64)],
) -> Result<Vec<SizeChange>, String> {
    if current.is_empty() || baseline.is_empty() {
        return Err("reports must contain at least one LTO mode".to_string());
    }

    if has_duplicate_modes(current) || has_duplicate_modes(baseline) {
        return Err("each report must contain each LTO mode once".to_string());
    }

    if current.len() != baseline.len()
        || baseline
            .iter()
            .any(|(baseline_lto, _)| {
                !current
                    .iter()
                    .any(|(current_lto, _)| current_lto == baseline_lto)
            })
    {
        return Err("baseline and current reports must contain the same LTO modes".to_string());
    }

    baseline
        .iter()
        .map(|(lto, baseline_bytes)| {
            if *baseline_bytes == 0 {
                return Err("baseline executable size must be greater than zero".to_string());
            }
            let current_bytes = current
                .iter()
                .find_map(|(current_lto, bytes)| (current_lto == lto).then_some(*bytes))
                .ok_or_else(|| {
                    "baseline and current reports must contain the same LTO modes".to_string()
                })?;
            let change_bytes = i128::from(current_bytes) - i128::from(*baseline_bytes);
            let change_percent = change_bytes as f64 / *baseline_bytes as f64 * 100.0;
            Ok(SizeChange {
                lto: *lto,
                baseline_bytes: *baseline_bytes,
                current_bytes,
                change_bytes,
                change_percent,
            })
        })
        .collect()
}

fn has_duplicate_modes(sizes: &[(Lto, u64)]) -> bool {
    sizes
        .iter()
        .enumerate()
        .any(|(index, (lto, _))| {
            sizes[index + 1..]
                .iter()
                .any(|(other_lto, _)| other_lto == lto)
        })
}

pub fn render_comparison(current: &str, baseline: &str) -> Result<String, String> {
    let current: SizeReport = serde_json::from_str(current).map_err(|error| error.to_string())?;
    let baseline: SizeReport = serde_json::from_str(baseline).map_err(|error| error.to_string())?;
    if current.target != baseline.target {
        return Err(format!(
            "cannot compare reports for different targets: {} and {}",
            baseline.target, current.target,
        ));
    }
    let feature_summary = format!(
        "Baseline features: `{}`; current features: `{}`\n\n",
        baseline.probe_features, current.probe_features,
    );
    let current_sizes: Vec<_> = current.measurements
        .iter()
        .map(|measurement| (measurement.lto, measurement.stripped_size))
        .collect();
    let baseline_sizes: Vec<_> = baseline.measurements
        .iter()
        .map(|measurement| (measurement.lto, measurement.stripped_size))
        .collect();
    let changes = compare_sizes(&current_sizes, &baseline_sizes)?;
    let mut output = feature_summary;
    output.push_str(
        "| LTO | Baseline | Current | Change | Change % |\n| --- | ---: | ---: | ---: | ---: |\n",
    );
    for change in changes {
        use std::fmt::Write as _;
        let _ = writeln!(
            output,
            "| {} | {} | {} | {} | {:+.2}% |",
            change.lto.as_str(),
            change.baseline_bytes,
            change.current_bytes,
            change.change_bytes,
            change.change_percent,
        );
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
