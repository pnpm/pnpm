use super::{NodeLinker, PackError, Value};
use pnpm_package_manifest::is_truthy;

pub(super) fn prevent_bundled_dependencies_with_pnp(
    node_linker: NodeLinker,
    manifest: &Value,
) -> Result<(), PackError> {
    if node_linker != NodeLinker::Pnp {
        return Ok(());
    }
    for field in ["bundledDependencies", "bundleDependencies"] {
        if manifest.get(field).is_some_and(is_truthy) {
            return Err(PackError::BundledDependenciesWithPnp {
                field,
                node_linker: node_linker_str(node_linker),
            });
        }
    }
    Ok(())
}

fn node_linker_str(node_linker: NodeLinker) -> &'static str {
    match node_linker {
        NodeLinker::Isolated => "isolated",
        NodeLinker::Hoisted => "hoisted",
        NodeLinker::Pnp => "pnp",
    }
}
