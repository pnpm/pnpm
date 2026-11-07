use super::super::FreshInputs;
use crate::SkippedSnapshots;
use pnpm_deps_restorer::PlatformLockfile;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use std::collections::{BTreeMap, HashSet};

/// Which importers a selected install materializes, and the lockfile
/// closed over them.
///
/// Built twice: first without a skip set, to give the host probe and the
/// layout the snapshots they plan against, then over the skip set the
/// plan computed. The plan borrows the first closure's lockfile, so the
/// second is a separate value rather than a mutation of the first.
pub(in crate::install_with_fresh_lockfile) struct MaterializationScope {
    /// `None` when the built lockfile already is the closure: every
    /// importer materializes and the resolve pass walked no group this
    /// run leaves out.
    importer_ids: Option<HashSet<String>>,
    closure: Option<crate::MaterializationClosure>,
    /// The built lockfile's peer classification, shared by every walk of
    /// this install.
    pub(in crate::install_with_fresh_lockfile) groups: crate::GroupSelection,
}

/// The second closure, with the importers that anchor project links.
pub(in crate::install_with_fresh_lockfile) struct FinalScope {
    closure: Option<crate::MaterializationClosure>,
    groups: crate::GroupSelection,
    pub(in crate::install_with_fresh_lockfile) project_anchor_importer_ids: HashSet<String>,
}

impl MaterializationScope {
    pub(in crate::install_with_fresh_lockfile) fn initial(
        install: FreshInputs<'_>,
        is_hoisted: bool,
        built: &Lockfile,
    ) -> Self {
        let importer_ids = closure_importer_ids(install, is_hoisted, built);
        let groups = crate::GroupSelection::classify(
            built,
            install.included(),
            install.drivers.config.peer_edge_options(),
        )
        .linking_for(install.projects.supported_architectures);
        let closure = Self::build_closure(
            install,
            is_hoisted,
            built,
            importer_ids.as_ref(),
            &groups,
            &SkippedSnapshots::new(),
        );
        MaterializationScope { importer_ids, closure, groups }
    }

    pub(in crate::install_with_fresh_lockfile) fn lockfile<'l>(
        &'l self,
        built: &'l Lockfile,
    ) -> &'l Lockfile {
        self.closure.as_ref().map_or(built, |closure| &closure.lockfile)
    }

    pub(in crate::install_with_fresh_lockfile) fn finalize(
        &self,
        install: FreshInputs<'_>,
        is_hoisted: bool,
        built: &Lockfile,
        skipped: &SkippedSnapshots,
    ) -> FinalScope {
        let closure = Self::build_closure(
            install,
            is_hoisted,
            built,
            self.importer_ids.as_ref(),
            &self.groups,
            skipped,
        );
        let materialized: HashSet<String> = match &closure {
            Some(closure) => closure.importer_ids.clone(),
            None => built.importers
                .keys()
                .cloned()
                .collect(),
        };
        let project_anchor_importer_ids =
            project_anchor_importer_ids(install.projects.real_ids, &materialized);
        FinalScope { closure, groups: self.groups.clone(), project_anchor_importer_ids }
    }

    fn build_closure(
        install: FreshInputs<'_>,
        is_hoisted: bool,
        built: &Lockfile,
        importer_ids: Option<&HashSet<String>>,
        groups: &crate::GroupSelection,
        skipped: &SkippedSnapshots,
    ) -> Option<crate::MaterializationClosure> {
        let hoisted_prior = install.prior.lockfile.filter(|_| is_hoisted);
        match install.projects.selected_ids {
            Some(selected) => Some(crate::selected_materialization_closure(
                built,
                install.projects.lockfile_dir,
                selected,
                install.projects.real_ids,
                hoisted_prior,
                groups,
                skipped,
            )),
            None => importer_ids.map(|importer_ids| {
                crate::materialization_closure(
                    built,
                    install.projects.lockfile_dir,
                    importer_ids,
                    groups,
                    skipped,
                )
            }),
        }
    }
}

impl FinalScope {
    pub(in crate::install_with_fresh_lockfile) fn lockfile<'l>(
        &'l self,
        built: &'l Lockfile,
    ) -> &'l Lockfile {
        self.closure.as_ref().map_or(built, |closure| &closure.lockfile)
    }

    pub(in crate::install_with_fresh_lockfile) fn on_disk_projects<'l>(
        &'l self,
        built: &'l Lockfile,
        importer_manifests: &'l BTreeMap<String, &'l PackageManifest>,
    ) -> super::super::on_disk::OnDiskProjects<'l> {
        super::super::on_disk::OnDiskProjects {
            materialization_lockfile: self.lockfile(built),
            platform_lockfile: PlatformLockfile { lockfile: built, groups: &self.groups },
            importer_manifests,
            project_anchor_importer_ids: &self.project_anchor_importer_ids,
        }
    }
}

/// The importers the materialization closure walks, or `None` when the
/// built lockfile needs no narrowing.
///
/// Two things narrow it. A `--filter` selection picks the importers; a
/// `--prod` / `--dev` run picks their dependency groups. The resolve
/// pass walks every group to keep `pnpm-lock.yaml` complete, so this
/// closure is where the graph comes back down to what the run installs
/// (pnpm/pnpm#14912).
fn closure_importer_ids(
    install: FreshInputs<'_>,
    is_hoisted: bool,
    built: &Lockfile,
) -> Option<HashSet<String>> {
    let hoisted_prior = install.prior.lockfile.filter(|_| is_hoisted);
    materialization_importer_ids(
        install.projects.selected_ids,
        install.projects.real_ids,
        hoisted_prior,
        built,
    )
    .or_else(|| {
        // A platform variant's own snapshot is wired in by the closure.
        (install.resolve_widened_groups()
            || install.included().excludes_a_group()
            || pnpm_deps_restorer::has_variant_snapshots(built))
        .then(|| {
            built.importers
                .keys()
                .cloned()
                .collect()
        })
    })
}

/// The importers a selected install materializes. See
/// [`crate::selected_materialization_ids`] for `hoisted_prior`.
fn materialization_importer_ids(
    selected_importer_ids: Option<&HashSet<String>>,
    real_importer_ids: Option<&HashSet<String>>,
    hoisted_prior: Option<&Lockfile>,
    built_lockfile: &Lockfile,
) -> Option<HashSet<String>> {
    let selected_importer_ids = selected_importer_ids?;
    Some(crate::selected_materialization_ids(
        built_lockfile,
        selected_importer_ids,
        real_importer_ids,
        hoisted_prior,
    ))
}

/// The importers whose own project manifests anchor the link phase: the
/// importers in `materialized` that are also in `real_importer_ids`, or all
/// of them when it is `None`.
fn project_anchor_importer_ids(
    real_importer_ids: Option<&HashSet<String>>,
    materialized: &HashSet<String>,
) -> HashSet<String> {
    real_importer_ids.map_or_else(|| materialized.clone(), |real| materialized & real)
}
