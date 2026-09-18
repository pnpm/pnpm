use super::{build_std, ensure_workspace_directory, git, read_workspace_file};
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_config::Config;
use pnpm_network::redact_and_sanitize_multiline;
use std::{collections::BTreeMap, env, fs, io, path::Path, process::Command};

pub(super) fn has_git_dependencies(metadata: &str) -> Result<bool> {
    let sources = pnpm_cargo_resolver::git_dependency_sources(metadata)?;
    for source in &sources {
        git::validate_transport(source)?;
    }
    Ok(!sources.is_empty())
}

pub(super) fn has_source_overrides(root: &Path) -> Result<bool> {
    let directory = ensure_workspace_directory(root, &[])?;
    let (manifest, _) = read_workspace_file(&directory, "Cargo.toml")
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", root.join("Cargo.toml").display()))?;
    let document: toml::Table = toml::from_str(&manifest)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse {}", root.join("Cargo.toml").display()))?;
    let mut has_overrides = false;
    let patches = document.get("patch").and_then(toml::Value::as_table);
    for overrides in patches.into_iter().flat_map(|patches| patches.values()) {
        has_overrides |= has_override_sources(overrides)?;
    }
    if let Some(overrides) = document.get("replace") {
        has_overrides |= has_override_sources(overrides)?;
    }
    Ok(has_overrides)
}

fn has_override_sources(overrides: &toml::Value) -> Result<bool> {
    let Some(overrides) = overrides.as_table() else { return Ok(false) };
    for dependency in overrides.values() {
        let Some(url) = dependency.get("git").and_then(toml::Value::as_str) else { continue };
        let source = format!("git+{url}")
            .parse()
            .into_diagnostic()
            .wrap_err("parse Cargo source override")?;
        git::validate_transport(&source)?;
    }
    Ok(!overrides.is_empty())
}

pub(super) async fn resolve_with_cargo(config: &Config, root: &Path) -> Result<String> {
    if !pnpm_cargo_resolver::is_crates_io(config.cargo_index_url()) {
        return Err(miette::miette!(
            r#"Resolving Cargo git dependencies or source overrides requires the default Cargo index. Use an existing Cargo.lock with a Cargo index declared in "registries"."#,
        ));
    }
    let root = root.to_path_buf();
    let offline = config.offline;
    tokio::task::spawn_blocking(move || resolve_workspace(&root, offline)).await
        .into_diagnostic()
        .wrap_err("join Cargo dependency resolution")?
}

fn resolve_workspace(root: &Path, offline: bool) -> Result<String> {
    let output = resolution_command(root, offline)?
        .output()
        .into_diagnostic()
        .wrap_err("run cargo generate-lockfile")?;
    if !output.status.success() {
        let stderr = redact_and_sanitize_multiline(&String::from_utf8_lossy(&output.stderr));
        let stderr = stderr.trim();
        let root = root.display();
        return Err(miette::miette!("cargo generate-lockfile failed for {root}: {stderr}"));
    }
    let directory = ensure_workspace_directory(root, &[])?;
    read_workspace_file(&directory, "Cargo.lock")
        .map(|(contents, _)| contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", root.join("Cargo.lock").display()))
}

fn resolution_command(root: &Path, offline: bool) -> Result<Command> {
    let sysroot = build_std::sysroot(root)?;
    let mut command = Command::new(
        sysroot
            .join("bin")
            .join(format!("cargo{}", env::consts::EXE_SUFFIX)),
    );
    // Cargo discovers configuration from its working directory, not the manifest.
    // Use the toolchain directory so checkout-controlled executable helpers and
    // environment settings are never loaded; user Cargo home settings still apply.
    command
        .current_dir(&sysroot)
        .env(
            "RUSTC",
            sysroot
                .join("bin")
                .join(format!("rustc{}", env::consts::EXE_SUFFIX)),
        )
        .args(["generate-lockfile", "--manifest-path"])
        .arg(root.join("Cargo.toml"));
    let protocols = pnpm_git_fetcher::read_allowed_git_protocols(root)
        .into_diagnostic()
        .wrap_err("read Cargo Git transport policy")?;
    command.env("GIT_ALLOW_PROTOCOL", protocols);
    for (key, value) in resolution_settings(root)? {
        command
            .arg("--config")
            .arg(format!("{key}={value}"));
    }
    if offline {
        command.arg("--offline");
    }
    Ok(command)
}

fn resolution_settings(root: &Path) -> Result<BTreeMap<&'static str, toml::Value>> {
    let mut settings = BTreeMap::new();
    for ancestor in root.ancestors() {
        let Some(name) = config_name(ancestor)? else { continue };
        let directory = ensure_workspace_directory(ancestor, &[".cargo"])?;
        let (contents, _) = read_workspace_file(&directory, name)
            .into_diagnostic()
            .wrap_err_with(|| format!("read {}", directory.path.join(name).display()))?;
        let document: toml::Table = toml::from_str(&contents)
            .into_diagnostic()
            .wrap_err("parse Cargo resolution configuration")?;
        for (table, key) in [("unstable", "bindeps"), ("resolver", "incompatible-rust-versions")] {
            let Some(value) = document
                .get(table)
                .and_then(|table| table.get(key))
            else {
                continue;
            };
            let name = match (table, key) {
                ("unstable", "bindeps") => "unstable.bindeps",
                _ => "resolver.incompatible-rust-versions",
            };
            validate_setting(name, value)?;
            settings.entry(name).or_insert_with(|| value.clone());
        }
    }
    Ok(settings)
}

fn validate_setting(key: &str, value: &toml::Value) -> Result<()> {
    let valid = match key {
        "unstable.bindeps" => value.is_bool(),
        _ => matches!(value.as_str(), Some("allow" | "fallback")),
    };
    if valid {
        return Ok(());
    }
    Err(miette::miette!("invalid Cargo resolution setting {key}: {value}"))
}

pub(super) fn config_name(root: &Path) -> Result<Option<&'static str>> {
    for name in ["config", "config.toml"] {
        let path = root.join(".cargo").join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => return Ok(Some(name)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("inspect {}", path.display()));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
