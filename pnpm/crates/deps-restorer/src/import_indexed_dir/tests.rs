mod reporting;

mod store;

mod workspace;

mod runtimes;

mod builds;

mod installation;

mod links;

use super::ImportIndexedDirOpts;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

fn write_source(dir: &Path, rel: &str, contents: &[u8]) -> PathBuf {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create source parent");
    }
    fs::write(&path, contents).expect("write source file");
    path
}

fn cas_map(entries: &[(&str, PathBuf)]) -> HashMap<String, PathBuf> {
    entries.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

const FORCE_KEEP: ImportIndexedDirOpts =
    ImportIndexedDirOpts { force: true, keep_modules_dir: true, safe_to_skip: false };
const FORCE_ONLY: ImportIndexedDirOpts =
    ImportIndexedDirOpts { force: true, keep_modules_dir: false, safe_to_skip: false };
const FORCE_SHARED: ImportIndexedDirOpts =
    ImportIndexedDirOpts { force: true, keep_modules_dir: false, safe_to_skip: true };
// The shape the isolated linker uses for a shared slot: no force, since a warm slot is
// short-circuited by its marker before the import runs.
const SHARED: ImportIndexedDirOpts =
    ImportIndexedDirOpts { force: false, keep_modules_dir: false, safe_to_skip: true };
// The shape the isolated linker uses for a shared slot whose build was interrupted.
#[allow(dead_code, reason = "test option preset")]
const FORCE_SHARED_KEEP: ImportIndexedDirOpts =
    ImportIndexedDirOpts { force: true, keep_modules_dir: true, safe_to_skip: true };
