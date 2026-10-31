#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetNodeUse {
    Compatibility,
    Execution,
}

#[must_use]
pub fn target_node_version<'a>(
    use_kind: TargetNodeUse,
    config_node_version: Option<&'a str>,
    runtime_pin: Option<&'a str>,
    host_node_version: Option<&'a str>,
) -> Option<&'a str> {
    match use_kind {
        TargetNodeUse::Compatibility => config_node_version.or(runtime_pin).or(host_node_version),
        TargetNodeUse::Execution => runtime_pin.or(host_node_version),
    }
}

#[must_use]
pub fn target_node_major(
    use_kind: TargetNodeUse,
    config_node_version: Option<&str>,
    runtime_pin: Option<&str>,
    host_node_version: Option<&str>,
) -> Option<u32> {
    target_node_version(use_kind, config_node_version, runtime_pin, host_node_version)
        .and_then(crate::install_frozen_lockfile::parse_major_from_version)
}

#[cfg(test)]
mod tests;
