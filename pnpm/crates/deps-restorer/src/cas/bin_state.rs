use super::write_file;
use pnpm_cmd_shim::{
    Host, PackageBinSource, get_bins_from_package_manifest, is_safe_bin_name, remove_bin,
};
use std::{collections::BTreeSet, io, path::Path};

pub(super) fn reconcile_bins(sources: &[PackageBinSource], directory: &Path) -> io::Result<()> {
    let current: BTreeSet<_> = sources
        .iter()
        .flat_map(|source| {
            get_bins_from_package_manifest::<Host>(&source.manifest, &source.location)
                .into_iter()
                .map(|command| command.name)
        })
        .collect();
    let state = directory
        .parent()
        .expect("bin directory has a parent")
        .join(".pnpm")
        .join(".cas-bin-names.json");
    let previous: BTreeSet<String> = match std::fs::read(&state) {
        Ok(contents) => serde_json::from_slice(&contents)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
        Err(error) => return Err(error),
    };
    for name in previous.difference(&current) {
        if !is_safe_bin_name(name) {
            return Err(io::Error::other(format!("Invalid recorded CAS bin name: {name}")));
        }
        remove_bin(&directory.join(name))?;
    }
    std::fs::create_dir_all(state.parent().expect("bin state has a parent"))?;
    write_file(&state, &serde_json::to_vec(&current)?)
}
