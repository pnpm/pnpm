//! Picks the version a missing optional peer of the importer is hoisted at.

use super::{
    BTreeMap, CandidatePeer, CandidatePeers, HashMap, HashSet, ImporterHoistState, MissingPeer,
    MissingPeerInfo, ParentPkgAliases, PreferredVersions, ResolveImporterError, Resolver,
    accepts_optional_peer_candidate, get_hoistable_optional_peers_with_preferred_versions,
    hoist_peers,
    hoist_rounds::hoisted_dependency,
    missing_peers::{MissingPeerKind, classify_missing_peer},
    resolve_direct_dependency_detached,
};
use crate::{ParentChain, peer_dependents_dedupe::OptionalPeerScope};
use futures_util::future;
use node_semver::Version;
use pipe_trait::Pipe;
use pnpm_resolving_resolver_base::{SharedDependencyManifest, get_peer_version_range};
use std::cell::RefCell;

/// What a candidate check knows about the version a required peer is
/// hoisted at.
#[derive(Debug, Clone)]
enum RequiredPeerPick {
    Version(String),
    /// The peer stays missing, or its version is not resolved yet.
    Unknown,
    /// The peer's specifier does not resolve, so the peer would stay unmet.
    Unresolvable,
}

/// The required peers of optional peer candidates that are resolved only to
/// read their manifests, without adding them to the tree.
#[derive(Default)]
struct DetachedPeers {
    /// The version each `(name, specifier)` resolved to, `None` when it did
    /// not resolve.
    versions: HashMap<(String, String), Option<String>>,
    /// The specifier to resolve for each `(name, specifier)` whose version
    /// or peers are not known yet.
    pending: RefCell<BTreeMap<(String, String), String>>,
}

/// Picks the versions the required peers of a candidate's waves are hoisted
/// at, for one pass of the candidate selection.
struct RequiredPeerPicker<'a> {
    candidate_peers: &'a CandidatePeers<'a>,
    detached: &'a DetachedPeers,
    /// Each pick, by name and range.
    picked: RefCell<HashMap<(String, String), RequiredPeerPick>>,
}

/// What [`ImporterHoistState::hoistable_optional_peers`] picked.
pub(super) struct HoistableOptionalPeers {
    /// `peer_name → version` to hoist.
    pub(super) versions: BTreeMap<String, String>,
    /// Names whose every candidate failed [`accepts_optional_peer_candidate`].
    pub(super) rejected: HashSet<String>,
}

impl ImporterHoistState {
    /// The version [`Self::hoist_missing_required`] hoists the missing
    /// required peer `name@range` at. A specifier whose version or peers
    /// are not known yet is added to `picker.detached.pending`.
    fn hoisted_required_peer_version(
        &self,
        name: &str,
        range: &str,
        picker: &RequiredPeerPicker<'_>,
    ) -> RequiredPeerPick {
        let names = || std::iter::once(name);
        let preferred =
            self.ctx.preferred_versions_for_names(&self.selection.preferred_versions, names());
        let resolved_versions = self.ctx.run_resolved_versions(names());
        let missing = [(name.to_string(), MissingPeerInfo { range: range.to_string() })];
        let Some(specifier) =
            hoist_peers(&self.hoist_peers_options(&preferred, &resolved_versions), &missing)
                .remove(name)
        else {
            return RequiredPeerPick::Unknown;
        };
        let exact_version = specifier
            .split(" || ")
            .next()
            .filter(|version| version.parse::<Version>().is_ok())
            .map(str::to_string);
        if let Some(version) = &exact_version
            && picker.candidate_peers.contains(name, version)
        {
            return RequiredPeerPick::Version(version.clone());
        }
        let key = (name.to_string(), specifier);
        if let Some(version) = picker.detached.versions.get(&key) {
            return version
                .clone()
                .map_or(RequiredPeerPick::Unresolvable, RequiredPeerPick::Version);
        }
        let to_resolve = exact_version.clone().unwrap_or_else(|| key.1.clone());
        picker.detached.pending.borrow_mut().insert(key, to_resolve);
        exact_version.map_or(RequiredPeerPick::Unknown, RequiredPeerPick::Version)
    }

    /// The versions the missing required peers of a wave of packages
    /// are hoisted at, with each name's declarations classified the way
    /// [`Self::hoist_missing_required`] classifies them. `None` when a
    /// required peer would stay unmet, as its ranges do not merge or it
    /// does not resolve.
    fn required_peer_versions(
        &self,
        missing_of_wave: &[Vec<CandidatePeer>],
        picker: &RequiredPeerPicker<'_>,
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
            let pick = picker.picked
                .borrow_mut()
                .entry((name.to_string(), range))
                .or_insert_with_key(|(name, range)| {
                    self.hoisted_required_peer_version(name, range, picker)
                })
                .clone();
            match pick {
                RequiredPeerPick::Version(version) => versions.push((name.to_string(), version)),
                RequiredPeerPick::Unknown => {}
                RequiredPeerPick::Unresolvable => return None,
            }
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
    /// [`accepts_optional_peer_candidate`]. The required peers it brings in
    /// whose version or peers are not known are resolved, without being
    /// added to the tree, and the selection runs again.
    pub(super) async fn hoistable_optional_peers<Chain>(
        &self,
        resolver: &Chain,
    ) -> Result<HoistableOptionalPeers, ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        let candidate_peers = CandidatePeers::new(self.ctx.workspace());
        let mut detached = DetachedPeers::default();
        loop {
            let hoistable = self.pick_hoistable_optional_peers(&candidate_peers, &detached);
            let pending = detached.pending.take();
            if pending.is_empty() {
                return Ok(hoistable);
            }
            for (key, manifest) in self.resolve_detached_peers(resolver, pending).await? {
                let version = manifest
                    .as_deref()
                    .and_then(|manifest| manifest.get("version")?.as_str())
                    .map(str::to_string);
                if let (Some(manifest), Some(version)) = (&manifest, &version) {
                    candidate_peers.insert_detached(
                        format!("{}@{version}", key.0),
                        CandidatePeer::from_manifest(manifest),
                    );
                }
                detached.versions.insert(key, version);
            }
        }
    }

    /// The manifest each pending specifier resolves to, `None` when it does
    /// not resolve. A failure is not reported: the candidate that brought
    /// the peer in may never be hoisted.
    async fn resolve_detached_peers<Chain>(
        &self,
        resolver: &Chain,
        pending: BTreeMap<(String, String), String>,
    ) -> Result<Vec<((String, String), Option<SharedDependencyManifest>)>, ResolveImporterError>
    where
        Chain: Resolver + ?Sized,
    {
        let parent_pkg_aliases =
            ParentPkgAliases::root(self.dependencies.parent_pkg_aliases.clone());
        pending
            .into_iter()
            .map(|(key, to_resolve)| async {
                let result = resolve_direct_dependency_detached(
                    &self.ctx,
                    resolver,
                    hoisted_dependency(key.0.clone(), to_resolve),
                    &self.importer_id,
                    &parent_pkg_aliases,
                )
                .await?;
                let manifest = result.and_then(|result| result.package.manifest.clone());
                Ok::<_, ResolveImporterError>((key, manifest))
            })
            .pipe(future::try_join_all)
            .await
    }

    fn pick_hoistable_optional_peers(
        &self,
        candidate_peers: &CandidatePeers<'_>,
        detached: &DetachedPeers,
    ) -> HoistableOptionalPeers {
        let provided_peer_versions = self.hoisted_provider_peer_versions();
        let hoist_preferred = self.optional_peer_preferred_versions(&provided_peer_versions);
        let picker = RequiredPeerPicker {
            candidate_peers,
            detached,
            picked: RefCell::new(HashMap::default()),
        };
        let pick_required_peer_versions = |missing_of_wave: &[Vec<CandidatePeer>]| {
            self.required_peer_versions(missing_of_wave, &picker)
        };
        let rejected = RefCell::new(HashSet::default());
        let accepts_candidate = |name: &str, version: &str| {
            let accepted = accepts_optional_peer_candidate(
                candidate_peers,
                &pick_required_peer_versions,
                &provided_peer_versions,
                (name, version),
            );
            if !accepted {
                rejected.borrow_mut().insert(name.to_string());
            }
            accepted
        };
        let versions = get_hoistable_optional_peers_with_preferred_versions(
            &self.dependencies.all_missing_optional_peers,
            &hoist_preferred,
            self.hoist_root_deps(),
            &self.ctx.optional_peer_version_tiers(
                &self.selection.locked_versions,
                self.dependencies.all_missing_optional_peers.keys().map(String::as_str),
            ),
            &accepts_candidate,
        );
        let mut rejected = rejected.into_inner();
        rejected.retain(|name| !versions.contains_key(name));
        HoistableOptionalPeers { versions, rejected }
    }

    /// The preferred versions a missing optional peer may be hoisted at,
    /// limited by [`OptionalPeerScope`].
    fn optional_peer_preferred_versions(
        &self,
        provided_peer_versions: &HashMap<String, String>,
    ) -> PreferredVersions {
        let names = self.dependencies.all_missing_optional_peers.keys().map(String::as_str);
        let mut preferred =
            self.ctx.preferred_versions_for_names(&self.selection.preferred_versions, names);
        match self.policy.peers.dedupe_peer_dependents.optional_peer_scope() {
            OptionalPeerScope::Project => {
                self.retain_own_closure_versions(&mut preferred, provided_peer_versions);
            }
            OptionalPeerScope::Workspace => {}
        }
        preferred
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
