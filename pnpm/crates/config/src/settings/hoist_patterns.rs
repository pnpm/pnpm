/// The two hoist patterns as one value, for
/// [`Config::hoist_patterns_before_virtual_store_only`](super::Config::hoist_patterns_before_virtual_store_only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoistPatterns {
    pub hoist_pattern: Option<Vec<String>>,
    pub public_hoist_pattern: Option<Vec<String>>,
}
