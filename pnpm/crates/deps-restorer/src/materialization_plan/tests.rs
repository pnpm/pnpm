use super::{TargetNodeUse, target_node_major, target_node_version};

#[test]
fn compatibility_order_prefers_config_node_version() {
    assert_eq!(
        target_node_version(
            TargetNodeUse::Compatibility,
            Some("20.0.0"),
            Some("24.0.0"),
            Some("22.0.0"),
        ),
        Some("20.0.0"),
    );
    assert_eq!(
        target_node_major(
            TargetNodeUse::Compatibility,
            Some("20.0.0"),
            Some("24.0.0"),
            Some("22.0.0"),
        ),
        Some(20),
    );
}

#[test]
fn compatibility_order_falls_back_to_runtime_pin() {
    assert_eq!(
        target_node_version(TargetNodeUse::Compatibility, None, Some("24.0.0"), Some("22.0.0"),),
        Some("24.0.0"),
    );
    assert_eq!(
        target_node_major(TargetNodeUse::Compatibility, None, Some("24.0.0"), Some("22.0.0"),),
        Some(24),
    );
}

#[test]
fn compatibility_order_falls_back_to_host() {
    assert_eq!(
        target_node_version(TargetNodeUse::Compatibility, None, None, Some("22.0.0"),),
        Some("22.0.0"),
    );
    assert_eq!(
        target_node_major(TargetNodeUse::Compatibility, None, None, Some("22.0.0"),),
        Some(22),
    );
}

#[test]
fn execution_order_prefers_runtime_pin_and_ignores_config() {
    assert_eq!(
        target_node_version(
            TargetNodeUse::Execution,
            Some("20.0.0"),
            Some("24.0.0"),
            Some("22.0.0"),
        ),
        Some("24.0.0"),
    );
    assert_eq!(
        target_node_major(TargetNodeUse::Execution, Some("20.0.0"), Some("24.0.0"), Some("22.0.0"),),
        Some(24),
    );
}

#[test]
fn execution_order_falls_back_to_host_and_ignores_config() {
    assert_eq!(
        target_node_version(TargetNodeUse::Execution, Some("20.0.0"), None, Some("22.0.0"),),
        Some("22.0.0"),
    );
    assert_eq!(
        target_node_major(TargetNodeUse::Execution, Some("20.0.0"), None, Some("22.0.0"),),
        Some(22),
    );
}
