use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use indexmap::IndexMap;
use pretty_assertions::assert_eq;

use super::{
    AssembleReleasePlanOptions, DependencyField, DependencyUpdate, ManifestDependency,
    PlannedRelease, ReleaseCause, ReleasePlan, VersioningInvariantCode, WorkspaceProject,
    assemble_release_plan, check_versioning_invariants, materialize_workspace_range,
};
use crate::{
    intents::{ChangeIntent, IntentBumpType},
    ledger::{Ledger, LedgerEntry},
    settings::{EpicSettings, ReleaseBumpType, VersioningSettings},
};

fn make_project(name: &str, version: &str, deps: &[(&str, &str)]) -> WorkspaceProject {
    WorkspaceProject {
        root_dir: PathBuf::from(format!("/ws/{name}")),
        name: Some(name.to_string()),
        version: Some(version.to_string()),
        prod_dependencies: deps
            .iter()
            .map(|(alias, spec)| ManifestDependency {
                field: DependencyField::Dependencies,
                alias: (*alias).to_string(),
                spec: (*spec).to_string(),
            })
            .collect(),
    }
}

fn bump(value: &str) -> IntentBumpType {
    match value {
        "none" => IntentBumpType::None,
        "patch" => IntentBumpType::Patch,
        "minor" => IntentBumpType::Minor,
        "major" => IntentBumpType::Major,
        _ => panic!("unknown bump type in test fixture: {value}"),
    }
}

fn make_intent(id: &str, releases: &[(&str, &str)]) -> ChangeIntent {
    ChangeIntent {
        id: id.to_string(),
        file_path: PathBuf::from(format!("/ws/.changeset/{id}.md")),
        releases: releases
            .iter()
            .map(|(name, bump_type)| ((*name).to_string(), bump(bump_type)))
            .collect::<IndexMap<String, IntentBumpType>>(),
        summary: format!("summary of {id}"),
    }
}

fn ledger(entries: &[(&str, &[&str])]) -> Ledger {
    entries
        .iter()
        .map(|(key, ids)| {
            ((*key).to_string(), LedgerEntry::Ids(ids.iter().map(|id| (*id).to_string()).collect()))
        })
        .collect()
}

fn assemble(
    projects: &[WorkspaceProject],
    intents: &[ChangeIntent],
    consumed: &Ledger,
    versioning: Option<&VersioningSettings>,
) -> ReleasePlan {
    assemble_release_plan(
        projects,
        std::path::Path::new("/ws"),
        intents,
        consumed,
        versioning,
        &AssembleReleasePlanOptions::default(),
    )
    .expect("plan assembles")
}

fn release<'a>(plan: &'a ReleasePlan, name: &str) -> &'a PlannedRelease {
    plan.releases.iter().find(|release| release.name == name).expect("release is planned")
}

fn release_names(plan: &ReleasePlan) -> Vec<&str> {
    plan.releases.iter().map(|release| release.name.as_str()).collect()
}

fn on_lane(pkg_name: &str, tag: &str) -> VersioningSettings {
    VersioningSettings {
        lanes: IndexMap::from([(pkg_name.to_string(), tag.to_string())]),
        ..VersioningSettings::default()
    }
}

fn twins() -> [WorkspaceProject; 2] {
    [
        WorkspaceProject {
            root_dir: PathBuf::from("/ws/pnpm11/pnpm"),
            name: Some("pnpm".to_string()),
            version: Some("11.0.0".to_string()),
            prod_dependencies: Vec::new(),
        },
        WorkspaceProject {
            root_dir: PathBuf::from("/ws/pnpm/npm/pnpm"),
            name: Some("pnpm".to_string()),
            version: Some("12.0.0".to_string()),
            prod_dependencies: Vec::new(),
        },
    ]
}

fn project_at(name: &str, version: &str, dir: &str) -> WorkspaceProject {
    WorkspaceProject {
        root_dir: PathBuf::from(format!("/ws/{dir}")),
        name: Some(name.to_string()),
        version: Some(version.to_string()),
        prod_dependencies: Vec::new(),
    }
}

fn epic(lead: &str, packages: &[&str]) -> EpicSettings {
    EpicSettings {
        lead: lead.to_string(),
        packages: packages.iter().map(|selector| (*selector).to_string()).collect(),
    }
}

fn assemble_with_unpublished(
    projects: &[WorkspaceProject],
    intents: &[ChangeIntent],
    versioning: Option<&VersioningSettings>,
    unpublished: &[&str],
) -> ReleasePlan {
    assemble_release_plan(
        projects,
        std::path::Path::new("/ws"),
        intents,
        &Ledger::new(),
        versioning,
        &AssembleReleasePlanOptions {
            unpublished_dirs: unpublished.iter().map(|dir| (*dir).to_string()).collect(),
            ..AssembleReleasePlanOptions::default()
        },
    )
    .expect("plan assembles")
}

mod dependencies;

mod behavior;

mod workspace_settings;

mod files;

mod configuration;

mod manifests;
