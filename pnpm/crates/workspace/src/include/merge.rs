use yaml_serde::{Mapping, Value};

/// Merge the local sources into the root manifest's settings.
///
/// Ownership is already checked, so the only key two sources can share is
/// `catalogs`, whose catalogs each source owns separately.
pub(super) fn merge(mut merged: Mapping, fragments: impl IntoIterator<Item = Mapping>) -> Mapping {
    for fragment in fragments {
        for (key, value) in fragment {
            merge_entry(&mut merged, key, value);
        }
    }
    merged
}

fn merge_entry(merged: &mut Mapping, key: Value, value: Value) {
    let Some(Value::Mapping(existing)) = merged.get_mut(&key) else {
        merged.insert(key, value);
        return;
    };
    if let Value::Mapping(catalogs) = value {
        existing.extend(catalogs);
    }
}

pub(super) fn render(merged: Mapping) -> String {
    yaml_serde::to_string(&Value::Mapping(merged)).expect("a parsed YAML document serializes")
}
