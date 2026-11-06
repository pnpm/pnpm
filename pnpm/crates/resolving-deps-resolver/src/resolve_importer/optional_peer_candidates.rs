//! Picks the version a missing optional peer of the importer is hoisted at.

use super::{
    BTreeMap, CandidatePeer, CandidatePeers, HashMap, HashSet, ImporterHoistState, MissingPeer,
    MissingPeerInfo, PreferredVersions, accepts_optional_peer_candidate,
    get_hoistable_optional_peers_with_preferred_versions, hoist_peers,
    missing_peers::{MissingPeerKind, classify_missing_peer},
};
use crate::{ParentChain, peer_dependents_dedupe::OptionalPeerScope};
use node_semver::Version;
use pnpm_resolving_resolver_base::get_peer_version_range;
use std::cell::RefCell;

/// The version picked for each required peer `(name, range)`.
type RequiredPeerPicks = RefCell<HashMap<(String, String), Option<String>>>;

impl ImporterHoistState {
    /// The exact version [`Self::hoist_missing_required`] hoists the
    /// missing required peer `name@range` at, or `None` when the peer stays
    /// missing or resolves from a range.
    fn hoisted_required_peer_version(&self, name: &str, range: &str) -> Option<String> {
        let names = || std::iter::once(name);
        let preferred =
            self.ctx.preferred_versions_for_names(&self.selection.preferred_versions, names());
        let resolved_versions = self.ctx.run_resolved_versions(names());
        let missing = [(name.to_string(), MissingPeerInfo { range: range.to_string() })];
        let specifier =
            hoist_peers(&self.hoist_peers_options(&preferred, &resolved_versions), &missing)
                .remove(name)?;
        let version = specifier.split(" || ").next()?;
        version
            .parse::<Version>()
            .is_ok()
            .then(|| version.to_string())
    }

    /// The exact versions the missing required peers of a wave of packages
    /// are hoisted at, with each name's declarations classified the way
    /// [`Self::hoist_missing_required`] classifies them. `None` when a
    /// required peer's ranges do not merge, so the peer would stay unmet.
    /// `picked` caches each pick by name and range.
    fn required_peer_versions(
        &self,
        missing_of_wave: &[Vec<CandidatePeer>],
        picked: &RequiredPeerPicks,
    ) -> Option<Vec<(String, String)>> {
        let mut declarations_by_name: BTreeMap<&str, Vec<MissingPeer>> = BTreeMap::new();
        for peer in missing_of_wave.iter().flatten() {
            declarations_by_name
                .entry(&peer.name)
                .or_default()
                .push(MissingPeer {
                    wanted_range: get_peer_version_range(&peer.range),
                    raw_range: peer.range.clone(),
                    optional: peer.optional,
                    parents: ParentChain::default(),
                });
        }
        let mut versions = Vec::new();
        for (name, declarations) in declarations_by_name {
            let range = match classify_missing_peer(
                &declarations,
                self.policy.peers.auto_install_peers_from_highest_match,
            ) {
                MissingPeerKind::Required(range) => range,
                MissingPeerKind::Optional(_) => continue,
                MissingPeerKind::Unhoistable if declarations.iter().all(|peer| peer.optional) => {
                    continue;
                }
                MissingPeerKind::Unhoistable => return None,
            };
            let version = picked
                .borrow_mut()
                .entry((name.to_string(), range))
                .or_insert_with_key(|(name, range)| self.hoisted_required_peer_version(name, range))
                .clone();
            versions.extend(version.map(|version| (name.to_string(), version)));
        }
        Some(versions)
    }

    /// `alias → version` a package hoisted to the importer resolves its own
    /// peers from: the importer's direct dependencies first, then the
    /// workspace root's.
    fn hoisted_provider_peer_versions(&self) -> HashMap<String, String> {
        let mut versions = if self.policy.peers.resolve_peers_from_workspace_root {
            (*self.dependencies.workspace_root_dep_versions).clone()
        } else {
            HashMap::default()
        };
        versions.extend(self.direct_dep_versions());
        versions
    }

    /// `peer_name → version` for each missing optional peer a version in
    /// the graph can supply. A candidate is skipped unless
    /// [`accepts_optional_peer_candidate`].
    pub(super) fn hoistable_optional_peers(&self) -> BTreeMap<String, String> {
        let names = || self.dependencies.all_missing_optional_peers.keys().map(String::as_str);
        let mut hoist_preferred =
            self.ctx.preferred_versions_for_names(&self.selection.preferred_versions, names());
        let preferred_tiers =
            self.ctx.optional_peer_version_tiers(&self.selection.locked_versions, names());
        let provided_peer_versions = self.hoisted_provider_peer_versions();
        match self.policy.peers.dedupe_peer_dependents.optional_peer_scope() {
            OptionalPeerScope::Project => {
                self.retain_own_closure_versions(&mut hoist_preferred, &provided_peer_versions);
            }
            OptionalPeerScope::Workspace => {}
        }
        let workspace = self.ctx.workspace();
        let candidate_peers = CandidatePeers::new(workspace);
        let picked: RequiredPeerPicks = RefCell::new(HashMap::default());
        let pick_required_peer_versions = |missing_of_wave: &[Vec<CandidatePeer>]| {
            self.required_peer_versions(missing_of_wave, &picked)
        };
        let accepts_candidate = |name: &str, version: &str| {
            accepts_optional_peer_candidate(
                &candidate_peers,
                &pick_required_peer_versions,
                &provided_peer_versions,
                (name, version),
            )
        };
        get_hoistable_optional_peers_with_preferred_versions(
            &self.dependencies.all_missing_optional_peers,
            &hoist_preferred,
            self.hoist_root_deps(),
            &preferred_tiers,
            &accepts_candidate,
        )
    }

    /// Drops from `preferred` every version that neither the importer's
    /// own dependency closure nor `provided_versions` holds, so a version
    /// that only another importer resolves does not supply an optional
    /// peer.
    fn retain_own_closure_versions(
        &self,
        preferred: &mut PreferredVersions,
        provided_versions: &HashMap<String, String>,
    ) {
        let names: HashSet<&str> = self.dependencies.all_missing_optional_peers
            .keys()
            .map(String::as_str)
            .collect();
        let mut own_versions = self.ctx
            .workspace()
            .versions_reachable_from(self.dependencies.direct.iter().map(|dep| &*dep.id), &names);
        for (name, version) in provided_versions {
            if names.contains(name.as_str()) {
                own_versions
                    .entry(name.clone())
                    .or_default()
                    .insert(version.clone());
            }
        }
        preferred.retain(|name, selectors| {
            let Some(versions) = own_versions.get(name) else { return false };
            selectors.retain(|version, _| versions.contains(version));
            !selectors.is_empty()
        });
    }
}
