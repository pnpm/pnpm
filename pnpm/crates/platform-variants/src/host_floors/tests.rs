use super::{NodeAbi, parse_node_abi};

#[test]
fn parses_the_node_abi_probe() {
    assert_eq!(parse_node_abi("10 127\n"), (Some(10), Some(NodeAbi::Node(127))));
    assert_eq!(parse_node_abi("undefined 127\n"), (None, Some(NodeAbi::Node(127))));
}
