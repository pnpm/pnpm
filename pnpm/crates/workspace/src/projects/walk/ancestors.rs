use super::{Path, PathBuf};

struct AncestorEntry {
    path: PathBuf,
    handle: Option<same_file::Handle>,
}

/// Tracks the directories on the active walk stack and detects symlink cycles
/// without opening a `same_file::Handle` unless the walk crosses a symlink.
pub(super) struct AncestorStack {
    entries: Vec<AncestorEntry>,
}

impl AncestorStack {
    pub(super) fn new(walk_root: &Path) -> Self {
        Self { entries: vec![AncestorEntry { path: walk_root.to_path_buf(), handle: None }] }
    }

    pub(super) fn push_literal(&mut self, path: PathBuf) {
        self.entries.push(AncestorEntry { path, handle: None });
    }

    pub(super) fn push_directory(&mut self, path: &Path, is_symlink: bool) -> bool {
        if !is_symlink {
            self.entries.push(AncestorEntry { path: path.to_path_buf(), handle: None });
            return true;
        }
        let Ok(target_handle) = same_file::Handle::from_path(path) else {
            return false;
        };
        if self.contains_handle(&target_handle) {
            return false;
        }
        self.entries.push(AncestorEntry { path: path.to_path_buf(), handle: Some(target_handle) });
        true
    }

    fn contains_handle(&mut self, target_handle: &same_file::Handle) -> bool {
        for ancestor in &mut self.entries {
            if ancestor.handle.is_none() {
                ancestor.handle = same_file::Handle::from_path(&ancestor.path).ok();
            }
            if ancestor.handle.as_ref() == Some(target_handle) {
                return true;
            }
        }
        false
    }

    pub(super) fn pop(&mut self) {
        self.entries.pop();
    }
}
