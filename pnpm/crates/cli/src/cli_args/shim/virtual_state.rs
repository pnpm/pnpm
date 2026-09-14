use super::{
    MAX_VIRTUAL_SHIM_METADATA_BYTES, Path, PathBuf, VIRTUAL_SHIM_STATE_PREFIX, VirtualShimState,
    create_short_hash, fs, io, is_safe_bin_name, is_valid_old_npm_package_name,
};
use miette::{Context, IntoDiagnostic};
use std::io::Read as _;

pub(super) fn read_virtual_shim_state(path: &Path) -> miette::Result<Option<VirtualShimState>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("read virtual shim state from {}", path.display()));
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_VIRTUAL_SHIM_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .into_diagnostic()
        .wrap_err_with(|| format!("read virtual shim state from {}", path.display()))?;
    if bytes.len() as u64 > MAX_VIRTUAL_SHIM_METADATA_BYTES {
        let path_display = path.display();
        return Err(miette::miette!("Virtual shim state at {path_display} is too large"));
    }
    let state: VirtualShimState = serde_json::from_slice(&bytes)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse virtual shim state from {}", path.display()))?;
    if !is_valid_old_npm_package_name(&state.package) {
        let path_display = path.display();
        return Err(miette::miette!(
            "Virtual shim state at {path_display} has an invalid package owner",
        ));
    }
    if let Some(bin) = state.bins
        .iter()
        .find(|bin| !is_safe_bin_name(bin))
    {
        let path_display = path.display();
        return Err(miette::miette!(
            "Virtual shim state at {path_display} contains invalid bin name {bin:?}",
        ));
    }
    Ok(Some(state))
}

pub(crate) fn record_virtual_shim_state(
    bin_dir: &Path,
    package: &str,
    bins: &[String],
) -> miette::Result<()> {
    let path = virtual_shim_state_path(bin_dir, package);
    let state = VirtualShimState { package: package.to_string(), bins: bins.to_vec() };
    let bytes = serde_json::to_vec(&state).into_diagnostic().wrap_err("serialize virtual shims")?;
    pnpm_fs::write_atomic(&path, &bytes)
        .into_diagnostic()
        .wrap_err_with(|| format!("record virtual shims at {}", path.display()))
}

pub(super) fn remove_virtual_shim_state(bin_dir: &Path, package: &str) -> miette::Result<()> {
    let path = virtual_shim_state_path(bin_dir, package);
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("remove virtual shim state at {}", path.display()));
        }
    }
    Ok(())
}

pub(super) fn virtual_shim_state_path(bin_dir: &Path, package: &str) -> PathBuf {
    let file_name = format!("{VIRTUAL_SHIM_STATE_PREFIX}{}.json", create_short_hash(package));
    bin_dir.join(file_name)
}
