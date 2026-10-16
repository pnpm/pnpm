mod package_manager_source;

pub use package_manager_source::{PackageManagerSource, package_manager_source};

pub const TOOL_PROTOCOL: &str = "tool:";
pub const RUNTIME_PROTOCOL: &str = "runtime:";

#[must_use]
pub fn strip_tool_protocol(specifier: &str) -> Option<&str> {
    specifier.strip_prefix(TOOL_PROTOCOL)
}

#[cfg(test)]
mod tests;
