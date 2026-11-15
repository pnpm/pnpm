use std::{collections::HashMap, fs};

use pretty_assertions::assert_eq;

use super::{DependencyKind, ManifestProjector};
use crate::{TreeNodeId, pkg_info::ManifestSource};

#[test]
fn specifiers_and_projection_share_a_query_scoped_manifest_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("package.json");
    fs::write(
        &path,
        r#"{"license":"MIT","dependencies":{"target":"^1.0.0"},"peerDependencies":{"target":"~1.0.0"}}"#,
    )
    .unwrap();
    let node_id = TreeNodeId::Package("parent@1.0.0".parse().unwrap());
    let resolved = HashMap::from([(
        node_id.clone(),
        ManifestSource {
            path: dir.path().to_path_buf(),
            integrity: None,
            name: "parent".to_string(),
            version: "1.0.0".to_string(),
        },
    )]);
    let fields = ["license".to_string()];
    let reader = ManifestProjector::new(&fields, &resolved);
    assert_eq!(
        reader.dependency_specifier(&node_id, "target", DependencyKind::Dependency).as_deref(),
        Some("^1.0.0"),
    );
    fs::remove_file(&path).unwrap();
    assert_eq!(reader.project(&node_id).unwrap()["license"], "MIT");
    assert_eq!(
        reader.dependency_specifier(&node_id, "target", DependencyKind::Peer).as_deref(),
        Some("~1.0.0"),
    );
    let next_query = ManifestProjector::new(&fields, &resolved);
    assert_eq!(
        next_query.dependency_specifier(&node_id, "target", DependencyKind::Dependency),
        None,
    );
}
