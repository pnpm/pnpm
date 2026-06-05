use std::{path::Path, sync::Arc};

use super::PnpmfileHooks;

#[must_use]
pub fn find_pnpmfile(root: &Path) -> Option<std::path::PathBuf> {
    let candidates = [".pnpmfile.mjs", ".pnpmfile.cjs"];

    for name in candidates {
        let path = root.join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

#[must_use]
pub fn load_pnpmfile(root: &Path) -> Option<Arc<dyn PnpmfileHooks>> {
    let file = find_pnpmfile(root)?;
    Some(Arc::new(super::node_runtime::NodeJsHooks::new(file)))
}
