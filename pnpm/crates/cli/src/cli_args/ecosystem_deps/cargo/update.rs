use super::{
    super::cargo_manifest_ops::{
        DeclaredCargoDep, declared_cargo_deps, find_cargo_workspace_root, same_declared_dep,
    },
    CargoUpdateOptions, Reporter as CargoReporter, SelectedCargoDep, execute_cargo_update,
    select_cargo_deps_to_update,
};
use crate::ecosystem_install::InstallContext;
use miette::Result;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

struct CargoWorkspaceSelection {
    dep: DeclaredCargoDep,
    explicit_spec: Option<String>,
    named: bool,
}

type CargoWorkspaceUpdates = BTreeMap<PathBuf, Vec<CargoWorkspaceSelection>>;

pub(crate) async fn update_cargo_projects<ReporterType: CargoReporter + 'static>(
    project_dirs: &[PathBuf],
    context: InstallContext,
    options: &CargoUpdateOptions<'_>,
) -> Result<bool> {
    let updates = collect_cargo_workspace_updates(project_dirs, &context, options)?;
    let updated = !updates.is_empty();
    execute_cargo_workspace_updates::<ReporterType>(updates, context, options).await?;
    Ok(updated)
}

fn collect_cargo_workspace_updates(
    project_dirs: &[PathBuf],
    context: &InstallContext,
    options: &CargoUpdateOptions<'_>,
) -> Result<CargoWorkspaceUpdates> {
    let mut updates = BTreeMap::new();
    for project_dir in project_dirs {
        collect_project_cargo_updates(project_dir, context, options, &mut updates)?;
    }
    Ok(updates)
}

fn collect_project_cargo_updates(
    project_dir: &Path,
    context: &InstallContext,
    options: &CargoUpdateOptions<'_>,
    updates: &mut CargoWorkspaceUpdates,
) -> Result<()> {
    let manifest_path = project_dir.join("Cargo.toml");
    if !context.config.cargo.enabled || !manifest_path.is_file() {
        return Ok(());
    }
    let declared = declared_cargo_deps(project_dir, context.config.workspace_dir.as_deref())?;
    let selected = select_cargo_deps_to_update(&declared, options);
    if selected.is_empty() {
        return Ok(());
    }
    let root = find_cargo_workspace_root(project_dir, context.config.workspace_dir.as_deref());
    merge_selected_cargo_deps(updates.entry(root).or_default(), selected);
    Ok(())
}

fn merge_selected_cargo_deps(
    updates: &mut Vec<CargoWorkspaceSelection>,
    selected: Vec<SelectedCargoDep<'_>>,
) {
    for selection in selected {
        match updates.iter_mut().find(|existing| same_declared_dep(&existing.dep, selection.dep)) {
            Some(existing) => {
                if existing.explicit_spec.is_none() {
                    existing.explicit_spec = selection.explicit_spec.map(str::to_string);
                }
                existing.named |= selection.named;
            }
            None => updates.push(CargoWorkspaceSelection {
                dep: selection.dep.clone(),
                explicit_spec: selection.explicit_spec.map(str::to_string),
                named: selection.named,
            }),
        }
    }
}

async fn execute_cargo_workspace_updates<ReporterType: CargoReporter + 'static>(
    updates: CargoWorkspaceUpdates,
    context: InstallContext,
    options: &CargoUpdateOptions<'_>,
) -> Result<()> {
    for (cargo_root, selected) in updates {
        let manifest_path = cargo_root.join("Cargo.toml");
        let mut declared =
            declared_cargo_deps(&cargo_root, context.config.workspace_dir.as_deref())?;
        for selection in &selected {
            if !declared.iter().any(|declared| same_declared_dep(declared, &selection.dep)) {
                declared.push(selection.dep.clone());
            }
        }
        let selected: Vec<_> = selected
            .iter()
            .filter_map(|selection| {
                declared
                    .iter()
                    .find(|declared| same_declared_dep(declared, &selection.dep))
                    .map(|dependency| SelectedCargoDep {
                        dep: dependency,
                        explicit_spec: selection.explicit_spec.as_deref(),
                        named: selection.named,
                    })
            })
            .collect();
        execute_cargo_update::<ReporterType>(
            &cargo_root,
            &manifest_path,
            &declared,
            &selected,
            context.clone(),
            options,
        )
        .await?;
    }
    Ok(())
}
