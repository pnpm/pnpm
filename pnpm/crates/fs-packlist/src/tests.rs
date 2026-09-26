use super::{PacklistOptions, packlist, packlist_with_options};
use serde_json::json;
use std::{fs, path::Path};
use tempfile::tempdir;

fn touch(root: &Path, rel: &str) {
    write(root, rel, "");
}

fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

mod files;

mod runtime;

mod reporting;

mod manifests;

mod workspace_settings;

mod security;

mod behavior;
