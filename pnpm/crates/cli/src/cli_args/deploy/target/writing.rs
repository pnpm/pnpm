use super::{DeployFiles, Lockfile, Path, Value, WORKSPACE_MANIFEST_FILENAME, Write, fs, io};
use miette::{Context, IntoDiagnostic};

pub(in super::super) fn write_deploy_files(
    deploy_dir: &Path,
    deploy_files: &DeployFiles,
) -> miette::Result<()> {
    let mut manifest = serde_json::to_string_pretty(&deploy_files.manifest).into_diagnostic()?;
    manifest.push('\n');
    let lockfile = deploy_files.lockfile
        .to_yaml_string()
        .map_err(miette::Report::new)
        .wrap_err("serialize deployed lockfile")?;
    write_atomic(&deploy_dir.join(Lockfile::FILE_NAME), lockfile.as_bytes())
        .into_diagnostic()
        .wrap_err("write deployed lockfile")?;
    if let Some(workspace_manifest) = &deploy_files.workspace_manifest {
        write_atomic(
            &deploy_dir.join(WORKSPACE_MANIFEST_FILENAME),
            workspace_manifest_yaml(workspace_manifest).as_bytes(),
        )
        .into_diagnostic()
        .wrap_err("write deployed workspace manifest")?;
    }
    write_atomic(&deploy_dir.join("package.json"), manifest.as_bytes())
        .into_diagnostic()
        .wrap_err("write deployed package.json")?;
    Ok(())
}

pub(super) fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    if let Ok(metadata) = fs::metadata(path) {
        tmp
            .as_file()
            .set_permissions(metadata.permissions())?;
    }
    tmp
        .persist(path)
        .map_err(|error| error.error)?;
    Ok(())
}

pub(super) fn workspace_manifest_yaml(workspace_manifest: &Value) -> String {
    let mut out = String::new();
    let Some(object) = workspace_manifest.as_object() else {
        return out;
    };
    for field in ["patchedDependencies", "allowBuilds"] {
        let Some(values) = object.get(field).and_then(Value::as_object) else {
            continue;
        };
        out.push_str(field);
        out.push_str(":\n");
        for (key, value) in values {
            out.push_str("  ");
            out.push_str(&serde_json::to_string(key).unwrap_or_else(|_| format!("{key:?}")));
            out.push_str(": ");
            out.push_str(&serde_json::to_string(value).unwrap_or_else(|_| value.to_string()));
            out.push('\n');
        }
    }
    out
}
