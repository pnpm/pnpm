use super::{HashSet, LinkBinsError, Path};
use crate::link_bins::shim_writer::remove_if_exists;

pub(super) fn remove_unclaimed(
    name: &str,
    bins_dir: &Path,
    provided: &HashSet<String>,
) -> Result<(), LinkBinsError> {
    for name in removable_names(name, provided, cfg!(windows)) {
        let path = bins_dir.join(name);
        remove_if_exists(&path).map_err(|error| LinkBinsError::RemoveStaleBin { path, error })?;
    }
    Ok(())
}

pub(super) fn removable_names(
    name: &str,
    provided: &HashSet<String>,
    windows: bool,
) -> Vec<String> {
    let mut names = vec![name.to_owned()];
    if windows {
        names.extend(["cmd", "ps1", "exe"].map(|extension| format!("{name}.{extension}")));
    }
    names.retain(|name| {
        let name = normalize(name, windows);
        !(provided.contains(&name)
            || windows
                && [".cmd", ".ps1", ".exe"]
                    .iter()
                    .any(|suffix| {
                        name.strip_suffix(suffix)
                            .is_some_and(|base| provided.contains(base))
                    }))
    });
    names
}

pub(super) fn normalize(name: &str, windows: bool) -> String {
    if windows { name.to_lowercase() } else { name.to_owned() }
}
