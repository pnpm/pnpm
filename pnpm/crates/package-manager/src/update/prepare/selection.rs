use super::{
    super::{
        UpdateOptions, UpdateSeed,
        seed_policy::{
            importer_seed_policy, matching::selectors_match_ignored_direct, selected_seed_policy,
        },
        selectors::parse_selectors,
    },
    SelectedUpdatePreparation, UpdatePreparation, UpdateResources, declared_direct, merge_catalogs,
};
use pnpm_package_manifest::PackageManifest;

pub(in crate::update) fn selection_matches_ignored_direct(
    manifest: &PackageManifest,
    update: UpdateOptions<'_>,
    owned: &UpdateResources,
) -> bool {
    let selectors = parse_selectors(update.selection.packages);
    let direct = declared_direct(manifest, &owned.include_direct);
    selectors_match_ignored_direct(&selectors, &direct, update.config)
}

pub(super) fn should_reject_unmatched_selection(
    update: UpdateOptions<'_>,
    prepared: &SelectedUpdatePreparation,
) -> bool {
    update.selection.depth == 0
        && !update.selection.packages.is_empty()
        && prepared.work == super::SelectedUpdateWork::None
}

impl SelectedUpdatePreparation {
    pub(in crate::update) fn has_work(&self) -> bool {
        self.work == super::SelectedUpdateWork::Prepared
    }

    pub(in crate::update) fn take_seed(&mut self, update: UpdateOptions<'_>) -> UpdateSeed {
        UpdateSeed {
            policy: selected_seed_policy(
                update.version.patches,
                std::mem::take(&mut self.seed_policies),
                update.selection.depth,
            ),
            preferred_versions_override: std::mem::take(&mut self.preferred_versions_override),
            catalogs_override: self.catalogs_override.take(),
        }
    }

    pub(super) fn record_ignored_match(
        &mut self,
        manifest: &PackageManifest,
        update: UpdateOptions<'_>,
        owned: &UpdateResources,
    ) {
        if self.work == super::SelectedUpdateWork::None
            && selection_matches_ignored_direct(manifest, update, owned)
        {
            self.work = super::SelectedUpdateWork::IgnoredDirectMatch;
        }
    }

    pub(super) fn merge(&mut self, index: usize, importer_id: String, prepared: UpdatePreparation) {
        self.work = super::SelectedUpdateWork::Prepared;
        for (name, selectors) in prepared.preferred_versions_override {
            self.preferred_versions_override
                .entry(name)
                .or_default()
                .extend(selectors);
        }
        if !prepared.bump_targets.is_empty() {
            self.bump_targets.insert(importer_id.clone(), prepared.bump_targets);
        }
        if let Some(policy) = importer_seed_policy(prepared.seed_policy) {
            self.seed_policies.insert(importer_id, policy);
        }
        if prepared.persist_manifest {
            self.persist_indices.push(index);
        }
        merge_catalogs(&mut self.updated_catalogs, &prepared.updated_catalogs);
        if let Some(complete_catalogs) = prepared.catalogs_override {
            self.catalogs_override = Some(complete_catalogs);
        }
        if self.workspace_dir_for_catalogs.is_none() {
            self.workspace_dir_for_catalogs = prepared.workspace_dir_for_catalogs;
        }
    }
}
