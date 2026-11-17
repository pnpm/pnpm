use super::{InheritedPeerProvider, Rc, resolved_peer_providers};
use crate::tests::result_node;

#[test]
fn transitive_peer_providers_handle_deep_chains() {
    let provider = result_node("schema", "schema@1.0.0", &[], vec![]);
    let mut nodes =
        vec![result_node("leaf", "leaf@1.0.0", &["schema"], vec![Rc::clone(&provider)])];
    for index in 0..20_000 {
        let child = Rc::clone(nodes.last().expect("chain has a leaf"));
        nodes.push(result_node(&format!("layer-{index}"), "1.0.0", &["schema"], vec![child]));
    }

    let bindings =
        resolved_peer_providers(nodes.last().expect("chain has a root"), "schema", || None);
    for node in &nodes {
        node.dependencies.borrow_mut().clear();
    }
    assert_eq!(bindings.recorded.len(), 1);
    assert!(bindings.recorded.contains(&crate::node_locator(&provider)));
    match bindings.inherited {
        InheritedPeerProvider::NotRequired => {}
        InheritedPeerProvider::Missing | InheritedPeerProvider::Resolved(_) => {
            panic!("the leaf records the peer provider");
        }
    }
}
