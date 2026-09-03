use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::Arc;

use super::update_parent_index;
use crate::resolved_tree::ChildEdge;

fn edge(pkg_id: &str) -> ChildEdge {
    ChildEdge { alias: pkg_id.to_string(), pkg_id: Arc::from(pkg_id), optional: false }
}

fn parents_of<'index>(
    index: &'index HashMap<Arc<str>, HashSet<Arc<str>>>,
    pkg_id: &str,
) -> Vec<&'index str> {
    let mut parents: Vec<&str> =
        index.get(pkg_id).into_iter().flatten().map(AsRef::as_ref).collect();
    parents.sort_unstable();
    parents
}

#[test]
fn recording_the_same_edges_again_keeps_one_parent_entry() {
    let mut index = HashMap::default();
    let edges = [edge("child@1.0.0")];
    update_parent_index(&mut index, "parent@1.0.0", None, &edges);
    update_parent_index(&mut index, "parent@1.0.0", Some(&edges), &edges);
    assert_eq!(parents_of(&index, "child@1.0.0"), ["parent@1.0.0"]);
}

#[test]
fn replacing_children_drops_the_stale_reverse_edge() {
    let mut index = HashMap::default();
    let before = [edge("old@1.0.0"), edge("kept@1.0.0")];
    let after = [edge("kept@1.0.0"), edge("new@1.0.0")];
    update_parent_index(&mut index, "other@1.0.0", None, &before[..1]);
    update_parent_index(&mut index, "parent@1.0.0", None, &before);
    update_parent_index(&mut index, "parent@1.0.0", Some(&before), &after);
    assert_eq!(parents_of(&index, "old@1.0.0"), ["other@1.0.0"]);
    assert_eq!(parents_of(&index, "kept@1.0.0"), ["parent@1.0.0"]);
    assert_eq!(parents_of(&index, "new@1.0.0"), ["parent@1.0.0"]);
}
