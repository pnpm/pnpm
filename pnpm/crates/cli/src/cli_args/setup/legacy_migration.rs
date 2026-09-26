use miette::{Context, IntoDiagnostic};
use pnpm_config::GLOBAL_LAYOUT_VERSION;
use pnpm_global::scan_global_packages;
use pnpm_local_spec::LocalSpec;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use std::{fs, path::Path, process::Command};

const LEGACY_GLOBAL_LAYOUT: &str = "5";

/// Generates `pnpm add -g` specifiers for dependencies from a legacy global manifest.
pub(crate) fn legacy_global_add_specs(
    dependencies: &serde_json::Map<String, serde_json::Value>,
    already_installed: &std::collections::BTreeSet<String>,
    manifest_dir: Option<&Path>,
) -> Vec<String> {
    let mut specs = Vec::new();
    for (name, spec) in dependencies {
        if name == "pnpm" || name == "@pnpm/exe" || already_installed.contains(name) {
            continue;
        }
        let Some(spec) = spec
            .as_str()
            .filter(|spec| !spec.is_empty())
        else {
            continue;
        };
        let spec = match manifest_dir {
            Some(dir) => LocalSpec::parse_filesystem(spec, dir)
                .map_or_else(|| spec.to_string(), |local| local.render(None)),
            None => spec.to_string(),
        };
        specs.push(format!("{name}@{spec}"));
    }
    specs.sort();
    specs
}

fn installed_global_aliases(pnpm_home_dir: &Path) -> std::collections::BTreeSet<String> {
    let global_dir = pnpm_home_dir.join("global").join(GLOBAL_LAYOUT_VERSION);
    scan_global_packages(&global_dir)
        .unwrap_or_default()
        .into_iter()
        .flat_map(|package| package.aliases())
        .collect()
}

fn legacy_global_dependencies(
    pnpm_home_dir: &Path,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let manifest_path = pnpm_home_dir
        .join("global")
        .join(LEGACY_GLOBAL_LAYOUT)
        .join("package.json");
    let bytes = fs::read(manifest_path).ok()?;
    let manifest = serde_json::from_slice::<serde_json::Value>(&bytes).ok()?;
    manifest
        .get("dependencies")
        .and_then(serde_json::Value::as_object)
        .cloned()
}

fn run_migration_add(
    exec_path: &Path,
    pnpm_home_dir: &Path,
    specs: &[String],
) -> miette::Result<()> {
    let status = Command::new(exec_path)
        .arg("add")
        .arg("-g")
        .args(specs)
        .env("PNPM_HOME", pnpm_home_dir)
        .env("PATH", super::bin_prepended_path(pnpm_home_dir))
        .status()
        .into_diagnostic()
        .wrap_err("run the global package migration")?;
    if !status.success() {
        let code = status.code().map_or_else(|| "unknown".to_string(), |code| code.to_string());
        return Err(miette::miette!("Failed to migrate global packages (exit code {code})"));
    }
    Ok(())
}

/// Reinstalls packages recorded in the legacy global layout into the current global directory.
pub(crate) fn migrate_legacy_global_packages<Reporter: self::Reporter + 'static>(
    exec_path: &Path,
    pnpm_home_dir: &Path,
    prefix_dir: &Path,
) -> miette::Result<()> {
    let manifest_path = pnpm_home_dir
        .join("global")
        .join(LEGACY_GLOBAL_LAYOUT)
        .join("package.json");
    let Some(dependencies) = legacy_global_dependencies(pnpm_home_dir) else {
        return Ok(());
    };
    let specs = legacy_global_add_specs(
        &dependencies,
        &installed_global_aliases(pnpm_home_dir),
        manifest_path.parent(),
    );
    if specs.is_empty() {
        return Ok(());
    }
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: format!(
            "Migrating global packages from the previous pnpm layout: {}",
            specs.join(" "),
        ),
        prefix: prefix_dir.to_string_lossy().into_owned(),
    }));
    run_migration_add(exec_path, pnpm_home_dir, &specs)
}

/// Emits a warning when legacy global migration fails.
pub(crate) fn finish_legacy_migration<Reporter: self::Reporter>(
    dir: &Path,
    migrated: miette::Result<()>,
) {
    if let Err(error) = migrated {
        Reporter::emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: format!("Failed to migrate global packages: {error}"),
            prefix: dir.to_string_lossy().into_owned(),
        }));
    }
}

#[cfg(test)]
mod tests;
