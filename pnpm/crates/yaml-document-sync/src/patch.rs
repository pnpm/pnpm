use super::{Error, edits};
use serde_json::Value;
use yamlpath::{Document, Route};

#[derive(Default)]
struct Changes<'a> {
    additions: Vec<(Route<'a>, Value)>,
    removals: Vec<Route<'a>>,
    replacements: Vec<(Route<'a>, &'a Value)>,
}

pub(super) fn sync(
    document: &mut Document,
    original: &Value,
    target: &Value,
) -> Result<(), Box<Error>> {
    let mut changes = Changes::default();
    changes.collect(Route::default(), original, target);
    let mut pending = Vec::new();
    for (route, addition) in changes.additions {
        edits::append(document, &mut pending, &route, &addition)?;
    }
    edits::apply(document, pending)?;
    let pending = changes.removals
        .into_iter()
        .map(|route| {
            document
                .removal_span(&route)
                .map(|range| (range, String::new()))
        })
        .collect::<Result<_, _>>()
        .map_err(Error::from)?;
    edits::apply(document, pending)?;
    let mut pending = Vec::new();
    for (route, target) in changes.replacements {
        edits::replace(document, &mut pending, &route, target)?;
    }
    edits::apply(document, pending)
}

impl<'a> Changes<'a> {
    fn collect(&mut self, route: Route<'a>, original: &'a Value, target: &'a Value) {
        if original == target {
            return;
        }
        match (original, target) {
            (Value::Object(original), Value::Object(target))
                if !original.is_empty() && !target.is_empty() =>
            {
                self.collect_mapping(route, original, target);
            }
            (Value::Array(original), Value::Array(target))
                if !original.is_empty() && !target.is_empty() =>
            {
                self.collect_sequence(route, original, target);
            }
            _ => self.replacements.push((route, target)),
        }
    }
    fn collect_mapping(
        &mut self,
        route: Route<'a>,
        original: &'a serde_json::Map<String, Value>,
        target: &'a serde_json::Map<String, Value>,
    ) {
        let mut added = serde_json::Map::new();
        for (key, value) in target {
            if !original.contains_key(key) {
                added.insert(key.clone(), value.clone());
            }
        }
        for (key, old) in original {
            let child = route.with_key(key.as_str());
            match target.get(key) {
                Some(new) => self.collect(child, old, new),
                None => self.removals.push(child),
            }
        }
        if !added.is_empty() {
            self.additions.push((route, Value::Object(added)));
        }
    }
    fn collect_sequence(&mut self, route: Route<'a>, original: &'a [Value], target: &'a [Value]) {
        for (index, (old, new)) in original.iter().zip(target).enumerate() {
            self.collect(route.with_key(index), old, new);
        }
        for index in target.len()..original.len() {
            self.removals.push(route.with_key(index));
        }
        if target.len() > original.len() {
            self.additions.push((route, Value::Array(target[original.len()..].to_vec())));
        }
    }
}
