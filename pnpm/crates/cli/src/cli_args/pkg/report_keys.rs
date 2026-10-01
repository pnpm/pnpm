use std::collections::HashMap;

/// What the `pnpm -r pkg get` report can key one project by.
pub(super) struct ReportIdentity {
    /// The manifest's `name`, when it declares a string one.
    pub name: Option<String>,
    /// The project's directory, relative to the workspace root.
    pub dir_key: String,
}

/// Choose a distinct report key for every project, in input order.
///
/// A project is keyed by its name unless that would hide another project:
/// a project without a name, every project that shares its name with
/// another selected project, and a project whose name equals the directory
/// key another project is reported under are keyed by their directory instead. Directories
/// are distinct, so the keys are too, and a workspace whose names are unique
/// keeps its name keys.
pub(super) fn report_keys(identities: &[ReportIdentity]) -> Vec<String> {
    let mut keyed_by_dir = initially_keyed_by_dir(identities);
    move_names_shadowed_by_dirs(identities, &mut keyed_by_dir);
    identities
        .iter()
        .zip(keyed_by_dir)
        .map(|(identity, by_dir)| match (&identity.name, by_dir) {
            (Some(name), false) => name.clone(),
            _ => identity.dir_key.clone(),
        })
        .collect()
}

fn initially_keyed_by_dir(identities: &[ReportIdentity]) -> Vec<bool> {
    let mut name_counts = HashMap::<&str, usize>::new();
    for name in identities.iter().filter_map(|identity| identity.name.as_deref()) {
        *name_counts.entry(name).or_default() += 1;
    }
    identities
        .iter()
        .map(|identity| {
            identity.name
                .as_deref()
                .is_none_or(|name| name_counts[name] > 1)
        })
        .collect()
}

/// Move every name-keyed project whose name equals a directory key in use
/// to its own directory key. A moved project's directory key can in turn
/// shadow another name, so each move is followed up the same way.
fn move_names_shadowed_by_dirs(identities: &[ReportIdentity], keyed_by_dir: &mut [bool]) {
    let name_keyed: HashMap<&str, usize> = identities
        .iter()
        .zip(keyed_by_dir.iter())
        .enumerate()
        .filter(|(_, (_, by_dir))| !**by_dir)
        .filter_map(|(index, (identity, _))| Some((identity.name.as_deref()?, index)))
        .collect();
    let mut pending: Vec<usize> = (0..identities.len())
        .filter(|&index| keyed_by_dir[index])
        .collect();
    while let Some(index) = pending.pop() {
        if let Some(&shadowed) = name_keyed.get(identities[index].dir_key.as_str())
            && !keyed_by_dir[shadowed]
        {
            keyed_by_dir[shadowed] = true;
            pending.push(shadowed);
        }
    }
}

#[cfg(test)]
mod tests;
