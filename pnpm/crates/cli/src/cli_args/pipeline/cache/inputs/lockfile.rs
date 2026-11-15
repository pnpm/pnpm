use super::{
    Lockfile, Path, TaskCache, WantedLockfileSelection, create_hex_hash, create_hex_hash_from_file,
    io,
};

impl TaskCache {
    pub(in super::super) fn lockfile_hash(
        &self,
        lockfile_dir: &Path,
        selection: &WantedLockfileSelection,
    ) -> miette::Result<String> {
        let cache_key = (lockfile_dir.to_path_buf(), selection.clone());
        if let Some(hash) = self.lockfile_hashes
            .lock()
            .expect("lockfile-hashes lock is not poisoned")
            .get(&cache_key)
        {
            return Ok(hash.clone());
        }
        let hash = hash_wanted_lockfile(lockfile_dir, selection)?;
        self.lockfile_hashes
            .lock()
            .expect("lockfile-hashes lock is not poisoned")
            .insert(cache_key, hash.clone());
        Ok(hash)
    }
}

fn hash_wanted_lockfile(
    lockfile_dir: &Path,
    selection: &WantedLockfileSelection,
) -> miette::Result<String> {
    let Some((base_name, base_hash)) = select_base_lockfile_hash(lockfile_dir, selection)? else {
        return Ok("no-lockfile".to_string());
    };
    if !selection.merge_git_branch_lockfiles {
        return Ok(base_hash);
    }
    let dir_display = lockfile_dir.display();
    let branch_files = Lockfile::git_branch_lockfiles(lockfile_dir)
        .map_err(|error| miette::miette!("listing branch lockfiles in {dir_display}: {error}"))?;
    let mut parts = vec![format!("base:{base_name}={base_hash}")];
    for branch_path in branch_files {
        let hash = match create_hex_hash_from_file(&branch_path) {
            Ok(hash) => hash,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                let branch_display = branch_path.display();
                return Err(miette::miette!("hashing branch lockfile {branch_display}: {error}"));
            }
        };
        let name = branch_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        parts.push(format!("branch:{name}={hash}"));
    }
    if parts.len() == 1 {
        return Ok(base_hash);
    }
    Ok(create_hex_hash(&parts.join("\0")))
}

fn select_base_lockfile_hash<'a>(
    lockfile_dir: &Path,
    selection: &'a WantedLockfileSelection,
) -> miette::Result<Option<(&'a str, String)>> {
    for file_name in selection.read_order() {
        if !Lockfile::wanted_exists(lockfile_dir, file_name) {
            continue;
        }
        let path = lockfile_dir.join(file_name);
        match create_hex_hash_from_file(&path) {
            Ok(hash) => return Ok(Some((file_name, hash))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                let path_display = path.display();
                return Err(miette::miette!("hashing lockfile {path_display}: {error}"));
            }
        }
    }
    Ok(None)
}
