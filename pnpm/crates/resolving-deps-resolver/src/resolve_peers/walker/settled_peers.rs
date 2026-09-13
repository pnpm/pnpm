use super::{Arc, ChildrenWalk, NodeOutput, SettledPeers, external_peers_to_report};

impl SettledPeers {
    pub(super) fn node_output(self, walked: &mut ChildrenWalk) -> NodeOutput {
        NodeOutput {
            dep_path: self.dep_path,
            external_resolved_peers: Arc::new(external_peers_to_report(
                &self.all_resolved,
                &walked.children_map,
                walked.discovery_children.as_ref(),
            )),
            auto_install_resolved_peers: std::mem::take(
                &mut walked.outputs.auto_install_resolved_peers,
            ),
            missing_peers: self.all_missing,
            subtree_missing_by_pkg: self.subtree_missing_by_pkg,
        }
    }
}
