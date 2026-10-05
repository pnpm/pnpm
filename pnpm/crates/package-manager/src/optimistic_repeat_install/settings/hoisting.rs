use super::{Config, NodeLinker, WorkspaceState};

pub(crate) fn recorded_hoisting_limits_match(
    recorded: Option<&WorkspaceState>,
    config: &Config,
    linker: NodeLinker,
) -> bool {
    if linker != NodeLinker::Hoisted {
        return true;
    }
    recorded.and_then(|state| state.settings.hoisting_limits.as_ref())
        == recorded_hoisting_limits(config, linker).as_ref()
}

pub(super) fn recorded_hoisting_limits(config: &Config, linker: NodeLinker) -> Option<String> {
    (linker == NodeLinker::Hoisted).then(|| {
        serde_json::to_value(config.hoisting_limits)
            .expect("serializing hoisting limits never fails")
            .as_str()
            .expect("hoisting limits serialize as strings")
            .to_string()
    })
}
