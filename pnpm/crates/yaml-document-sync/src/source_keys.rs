use super::{Error, parse, scalar_aliases::paths::mapping_paths};
use serde_json::Value;
use std::collections::HashMap;
use yamlpath::{Component, Route};

type KeyNames = HashMap<String, HashMap<String, String>>;

#[derive(Default)]
pub(crate) struct SourceKeys {
    source: KeyNames,
    canonical: KeyNames,
}

impl SourceKeys {
    pub(crate) fn new(text: &str, original: &Value) -> Result<Self, Box<Error>> {
        let mut keys = Self::default();
        for mut path in mapping_paths(text)? {
            let Some(Component::Key(source)) = path.pop() else { continue };
            let Some(mapping) = keys.value_at(original, &path).and_then(Value::as_object) else {
                continue;
            };
            if mapping.contains_key(source.as_ref()) {
                continue;
            }
            let canonical = match parse(&source)? {
                Value::String(value) => value,
                value => value.to_string(),
            };
            if !mapping.contains_key(&canonical) {
                continue;
            }
            let parent = route_id(&Route::from(path));
            keys.source
                .entry(parent.clone())
                .or_default()
                .insert(canonical.clone(), source.to_string());
            keys.canonical
                .entry(parent)
                .or_default()
                .insert(source.to_string(), canonical);
        }
        Ok(keys)
    }

    pub(crate) fn child<'a>(&self, parent: &Route<'a>, key: &'a str) -> Route<'a> {
        let source = self.source
            .get(&route_id(parent))
            .and_then(|keys| keys.get(key));
        match source {
            Some(source) => parent.with_key(source.clone()),
            None => parent.with_key(key),
        }
    }

    pub(crate) fn value_at<'a>(
        &self,
        mut value: &'a Value,
        path: &[Component<'_>],
    ) -> Option<&'a Value> {
        let mut parent = Route::default();
        for component in path {
            value = match component {
                Component::Key(key) => {
                    let canonical = self.canonical
                        .get(&route_id(&parent))
                        .and_then(|keys| keys.get(key.as_ref()));
                    value.get(canonical.map_or_else(|| key.as_ref(), String::as_str))?
                }
                Component::Index(index) => value.get(*index)?,
            };
            parent = parent.with_key(component.clone());
        }
        Some(value)
    }
}

fn route_id(route: &Route<'_>) -> String {
    serde_json::to_string(route).expect("serializing a YAML route cannot fail")
}
