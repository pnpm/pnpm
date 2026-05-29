use std::path::Path;
use std::sync::Arc;

use super::PnpmfileHooks;

pub fn find_pnpmfile(root: &Path) -> Option<std::path::PathBuf> {
    let candidates = [".pnpmfile.mjs", ".pnpmfile.cjs", "pnpmfile.cjs", "pnpmfile.mjs"];

    for name in candidates {
        let path = root.join(name);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

pub fn load_pnpmfile(root: &Path) -> Option<Arc<dyn PnpmfileHooks>> {
    let file = find_pnpmfile(root)?;
    Some(Arc::new(super::node_runtime::NodeJsHooks { file }))
}
