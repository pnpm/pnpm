use super::{
    LinkRoots, LinkedDependency, LinkedLockfileLoader, Lockfile, PackageManifest, Path, PathBuf,
    ProjectManifests, manifest_str_field, package_manifest_version,
};
use std::{borrow::Cow, collections::HashMap, path::Component, sync::Arc};

/// The linked dependencies one walk has read. Most importers of a workspace
/// link the same packages, so each target is canonicalized and its manifest
/// read once per walk rather than once per importer that links it.
pub(crate) struct LinkedPackages<'r, 'm, 'b, 'l> {
    roots: &'r LinkRoots<'r>,
    project_manifests: &'m ProjectManifests<'m>,
    consuming_lockfile: Option<&'r Lockfile>,
    linked_lockfiles: Option<&'b mut LinkedLockfileLoader<'l>>,
    key_base_by_importer_dir: HashMap<PathBuf, Option<PathBuf>>,
    pub(crate) by_target: HashMap<PathBuf, Option<LinkedDependency<'m>>>,
}

impl<'r, 'm, 'b, 'l> LinkedPackages<'r, 'm, 'b, 'l> {
    pub(crate) fn new(
        roots: &'r LinkRoots<'r>,
        project_manifests: &'m ProjectManifests<'m>,
        consuming_lockfile: Option<&'r Lockfile>,
        linked_lockfiles: Option<&'b mut LinkedLockfileLoader<'l>>,
    ) -> Self {
        Self {
            roots,
            project_manifests,
            consuming_lockfile,
            linked_lockfiles,
            key_base_by_importer_dir: HashMap::new(),
            by_target: HashMap::new(),
        }
    }

    /// The version, the peer check, and the linked importer's own snapshot
    /// all need the same answers. Targets escaping both the workspace boundary
    /// and the lockfile directory are rejected.
    pub(crate) fn resolve(
        &mut self,
        importer_dir: &Path,
        link_target: &str,
    ) -> Option<&LinkedDependency<'m>> {
        let target = self.target_key(importer_dir, Path::new(link_target));
        let canonical_lockfile_dir = self.roots.canonical_lockfile_dir()?;
        let roots = self.roots;
        let project_manifests = self.project_manifests;
        let consuming_lockfile = self.consuming_lockfile;
        let linked_lockfiles = &mut self.linked_lockfiles;
        self.by_target
            .entry(target)
            .or_insert_with_key(|target| {
                build_linked_dependency(
                    roots,
                    canonical_lockfile_dir,
                    project_manifests,
                    consuming_lockfile,
                    linked_lockfiles.as_deref_mut(),
                    target,
                )
            })
            .as_ref()
    }

    /// `..` is popped off the importer directory, so every importer
    /// reaching one package shares its entry:
    /// - Unix resolves `..` physically, so it is popped off the canonical
    ///   importer directory.
    /// - Windows resolves `..` lexically on the spelled path, so it is popped
    ///   only when that path is already canonical.
    ///
    /// Any other importer keeps the path it spells.
    fn target_key(&mut self, importer_dir: &Path, link_target: &Path) -> PathBuf {
        let base = self.key_base_by_importer_dir
            .entry(importer_dir.to_path_buf())
            .or_insert_with_key(|dir| {
                let canonical = dunce::canonicalize(dir).ok()?;
                (cfg!(unix) || canonical == *dir).then_some(canonical)
            });
        let Some(base) = base else {
            return importer_dir.join(link_target);
        };
        let mut target = base.clone();
        let mut components = link_target.components().peekable();
        while components.next_if_eq(&Component::ParentDir).is_some() {
            target.pop();
        }
        target.extend(components);
        target
    }
}

fn build_linked_dependency<'m>(
    roots: &LinkRoots<'_>,
    canonical_lockfile_dir: &Path,
    project_manifests: &'m ProjectManifests<'m>,
    consuming_lockfile: Option<&Lockfile>,
    linked_lockfiles: Option<&mut LinkedLockfileLoader<'_>>,
    target: &Path,
) -> Option<LinkedDependency<'m>> {
    let dir = roots.canonical_target(target)?;
    let importer_id = pnpm_workspace::importer_id_from_root_dir(canonical_lockfile_dir, &dir);
    let manifest = linked_manifest(project_manifests, &importer_id, &dir);
    let lockfile = load_sibling_lockfile(
        consuming_lockfile,
        linked_lockfiles,
        &importer_id,
        &dir,
        manifest.as_deref(),
    );
    let version = manifest
        .as_deref()
        .and_then(package_manifest_version)
        .unwrap_or_else(|| "0.0.0".to_string());
    Some(LinkedDependency { dir, importer_id, manifest, lockfile, version })
}

fn load_sibling_lockfile(
    consuming_lockfile: Option<&Lockfile>,
    linked_lockfiles: Option<&mut LinkedLockfileLoader<'_>>,
    importer_id: &str,
    dir: &Path,
    manifest: Option<&PackageManifest>,
) -> Option<Arc<Lockfile>> {
    if consuming_lockfile.is_some_and(|lockfile| lockfile.importers.contains_key(importer_id)) {
        return None;
    }
    let pkg_name = manifest.and_then(|manifest| manifest_str_field(manifest, "name"));
    linked_lockfiles.and_then(|load| load(dir, pkg_name))
}

/// The manifest of the linked directory `dir`, taken from
/// [`ProjectManifests`] when `importer_id` names a loaded project.
pub(super) fn linked_manifest<'m>(
    project_manifests: &ProjectManifests<'m>,
    importer_id: &str,
    dir: &Path,
) -> Option<Cow<'m, PackageManifest>> {
    match project_manifests.get(importer_id) {
        Some(&manifest) => Some(Cow::Borrowed(manifest)),
        None => pnpm_workspace::read_project_manifest_only(dir).ok().map(Cow::Owned),
    }
}
