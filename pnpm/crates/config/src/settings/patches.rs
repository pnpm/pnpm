use crate::{
    BTreeMap,
    CalcPatchHashError,
    Config,
    IndexMap,
    PatchGroupRecord,
    PatchInput,
    Path,
    ResolvePatchedDependenciesError,
    create_hex_hash_from_file,
    group_patched_dependencies,
    resolve_and_group,
};

impl Config {
    pub fn resolved_patched_dependencies(
        &self,
    ) -> Result<Option<PatchGroupRecord>, ResolvePatchedDependenciesError> {
        if let Some(hashes) = self.patched_dependency_hashes_override.as_ref() {
            let groups = group_patched_dependencies(
                hashes
                    .iter()
                    .map(|(key, hash)| {
                        (key.clone(), PatchInput { hash: hash.clone(), patch_file_path: None })
                    }),
            )?;
            return Ok((!groups.is_empty()).then_some(groups));
        }
        let (Some(workspace_dir), Some(raw)) = (&self.workspace_dir, &self.patched_dependencies)
        else {
            return Ok(None);
        };
        resolve_and_group(workspace_dir, raw)
    }

    /// Resolve relative patch file paths in
    /// [`Config::patched_dependencies`] against
    /// [`Config::workspace_dir`] and hash each file, producing the
    /// `patchedDependencies` map the lockfile records: each configured
    /// key mapped to its patch file's SHA-256 hex digest.
    ///
    /// Distinct from [`Self::resolved_patched_dependencies`], which
    /// groups the same entries by package name for the resolver — this
    /// keeps the user's verbatim keys so the lockfile is byte-faithful
    /// (e.g. a bare `foo` and `foo@*` stay separate keys rather than
    /// collapsing into one group bucket).
    ///
    /// Returns `Ok(None)` when either field is unset.
    pub fn patched_dependency_hashes(
        &self,
    ) -> Result<Option<BTreeMap<String, String>>, CalcPatchHashError> {
        Ok(self
            .patched_dependency_hashes_in_config_order()?
            .map(|hashes| hashes.into_iter().collect()))
    }

    /// Return patch hashes in configured selector order.
    ///
    /// Precomputed overrides avoid file reads. Without an override, each
    /// configured patch file is hashed and any I/O or hashing error is
    /// propagated. Returns `None` when no non-empty patch configuration is
    /// available.
    pub fn patched_dependency_hashes_in_config_order(
        &self,
    ) -> Result<Option<IndexMap<String, String>>, CalcPatchHashError> {
        if let Some(hashes) = self.patched_dependency_hashes_override.as_ref() {
            return Ok((!hashes.is_empty()).then(|| hashes.clone()));
        }
        let (Some(workspace_dir), Some(raw)) = (&self.workspace_dir, &self.patched_dependencies)
        else {
            return Ok(None);
        };
        let mut hashes = IndexMap::with_capacity(raw.len());
        for (key, rel_or_abs) in raw {
            let candidate = Path::new(rel_or_abs);
            let path = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                workspace_dir.join(candidate)
            };
            hashes.insert(key.clone(), create_hex_hash_from_file(&path)?);
        }
        Ok((!hashes.is_empty()).then_some(hashes))
    }
}
