use super::{
    Manifest,
    Workspace,
};
use miette::{
    Result,
    bail,
};
use pep508_rs::{
    ExtraName,
    PackageName,
};
use pnpm_python_resolver::parse_requirement;
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

impl Workspace {
    pub(crate) fn resolution_manifest(
        &self,
        root: &Path,
        manifest: &Arc<Manifest>,
    ) -> Arc<Manifest> {
        Arc::clone(self.inherited.get(root).map_or(manifest, |(_, manifest)| manifest))
    }

    pub(super) fn selected_distributions(
        &self,
        manifest: &Manifest,
        extras: Option<&BTreeSet<ExtraName>>,
    ) -> Result<BTreeMap<PackageName, BTreeSet<ExtraName>>> {
        let Some(selection) = &self.selection else {
            return Ok(manifest
                .declared_distributions()?
                .into_iter()
                .map(|name| (name, BTreeSet::new()))
                .collect());
        };
        let requirements = match extras {
            None => {
                manifest.requirements(selection.config, crate::manifest::DependencySelection::ALL)?
            }
            Some(_) => manifest
                .distribution_requirements()?
                .iter()
                .map(|requirement| parse_requirement(requirement))
                .collect::<Result<Vec<_>>>()?,
        };
        let extras = extras
            .map(|extras| {
                extras
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut names = BTreeMap::<PackageName, BTreeSet<ExtraName>>::new();
        for requirement in requirements {
            if selection.environments
                .iter()
                .any(|environment| requirement.marker.evaluate(environment, &extras))
            {
                names
                    .entry(requirement.name)
                    .or_default()
                    .extend(requirement.extras);
            }
        }
        Ok(names)
    }
}

pub(super) struct Target {
    pub(super) root: PathBuf,
    pub(super) editable: bool,
    pub(super) extras: BTreeSet<ExtraName>,
}

impl Target {
    pub(super) fn merge(&mut self, target: &Self, name: &PackageName, root: &Path) -> Result<bool> {
        if self.root != target.root {
            bail!(
                "two Python sources reachable from {} give `{name}`: {} and {}",
                root.display(),
                self.root.display(),
                target.root.display(),
            );
        }
        if self.editable != target.editable {
            bail!(
                "two Python sources reachable from {} install `{name}` from {} differently: one editable, one not",
                root.display(),
                target.root.display(),
            );
        }
        let changed = !target.extras.is_subset(&self.extras);
        self.extras.extend(target.extras.iter().cloned());
        Ok(changed)
    }
}
