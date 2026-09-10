use super::{Map, PkgError, Segment, Value, parse_property_path};

const UNSAFE_KEYS: [&str; 3] = ["__proto__", "constructor", "prototype"];

pub(super) fn check_unsafe_key_in_path(key: &str) -> Result<(), PkgError> {
    let segments = parse_property_path(key).map_err(PkgError::InvalidPropertyPath)?;
    for segment in &segments {
        if let Segment::Key(k) = segment
            && UNSAFE_KEYS.contains(&k.as_str())
        {
            return Err(PkgError::UnsafeKey { key: k.clone() });
        }
    }
    Ok(())
}

pub(crate) const MAX_ARRAY_INDEX: usize = 1 << 20;

fn validate_index(idx: f64) -> Result<usize, PkgError> {
    if idx.fract() != 0.0 || idx.is_sign_negative() || !idx.is_finite() {
        return Err(PkgError::SetPathError { path: idx.to_string() });
    }
    let index = idx as usize;
    if index > MAX_ARRAY_INDEX {
        return Err(PkgError::SetPathError { path: idx.to_string() });
    }
    Ok(index)
}

fn idx_to_string(idx: f64) -> String {
    if idx.fract() == 0.0 && idx.is_finite() { format!("{}", idx as i64) } else { idx.to_string() }
}

pub(super) fn set_object_value_by_property_path(
    root: &mut Value,
    path: &str,
    value: Value,
) -> miette::Result<()> {
    if path.is_empty() {
        return Err(PkgError::EmptyPath.into());
    }
    check_unsafe_key_in_path(path)?;
    let segments = parse_property_path(path)
        .map_err(|err| miette::Report::new(PkgError::InvalidPropertyPath(err)))?;
    let Some((last, parents)) = segments.split_last() else {
        return Err(PkgError::EmptyPath.into());
    };
    let mut current = root;
    for (position, segment) in parents.iter().enumerate() {
        // A container is created — or replaced — to match what the next
        // segment addresses it as, so a path can be set into a document
        // that does not describe it yet.
        let needs_array = matches!(&segments[position + 1], Segment::Index(_));
        current = descend_for_set(current, segment, needs_array)?;
    }
    place_value(current, last, value)
}

/// Step into the container `segment` names, creating it when the
/// document has nothing there or something of the wrong shape.
fn descend_for_set<'a>(
    current: &'a mut Value,
    segment: &Segment,
    needs_array: bool,
) -> miette::Result<&'a mut Value> {
    let key = match segment {
        Segment::Key(key) => key.clone(),
        Segment::Index(idx) => {
            let index = validate_index(*idx)?;
            if current.is_array() {
                return Ok(descend_array(current, index, needs_array));
            }
            if !current.is_object() {
                // Neither container the index can address, so it is
                // replaced by one; the walk continues from there.
                *current = index_container(*idx, index, needs_array);
                return Ok(current);
            }
            idx_to_string(*idx)
        }
    };
    Ok(descend_object(current, &key, needs_array))
}

fn descend_array(current: &mut Value, index: usize, needs_array: bool) -> &mut Value {
    let arr = current.as_array_mut().expect("the caller checked the value is an array");
    if index >= arr.len() {
        arr.resize(index.saturating_add(1), Value::Null);
    }
    if !container_matches(&arr[index], needs_array) {
        arr[index] = empty_container(needs_array);
    }
    &mut arr[index]
}

fn descend_object<'a>(current: &'a mut Value, key: &str, needs_array: bool) -> &'a mut Value {
    if !current.is_object() {
        *current = Value::Object(Map::new());
    }
    let obj = current.as_object_mut().expect("current was just made an object");
    if !obj.get(key).is_some_and(|entry| container_matches(entry, needs_array)) {
        obj.insert(key.to_owned(), empty_container(needs_array));
    }
    obj.get_mut(key).expect("the entry was just inserted")
}

/// Write the value at the path's last segment.
fn place_value(current: &mut Value, last: &Segment, value: Value) -> miette::Result<()> {
    let key = match last {
        Segment::Key(key) => key.clone(),
        Segment::Index(idx) => {
            let index = validate_index(*idx)?;
            if !current.is_object() {
                place_at_index(current, index, value);
                return Ok(());
            }
            idx_to_string(*idx)
        }
    };
    if !current.is_object() {
        *current = Value::Object(Map::new());
    }
    current.as_object_mut().expect("current was just made an object").insert(key, value);
    Ok(())
}

/// Write into the array slot the index names, growing — or building —
/// the array to reach it.
fn place_at_index(current: &mut Value, index: usize, value: Value) {
    if !current.is_array() {
        *current = Value::Array(Vec::new());
    }
    let arr = current.as_array_mut().expect("current was just made an array");
    if index >= arr.len() {
        arr.resize(index.saturating_add(1), Value::Null);
    }
    arr[index] = value;
}

fn container_matches(value: &Value, needs_array: bool) -> bool {
    if needs_array { value.is_array() } else { value.is_object() }
}

fn empty_container(needs_array: bool) -> Value {
    if needs_array { Value::Array(Vec::new()) } else { Value::Object(Map::new()) }
}

/// The container an index segment builds when the document has
/// something else there: an array long enough to hold the index, or an
/// object keyed by the index's string form.
fn index_container(idx: f64, index: usize, needs_array: bool) -> Value {
    if needs_array {
        let mut arr = Vec::with_capacity(index.saturating_add(1));
        arr.resize(index.saturating_add(1), Value::Null);
        return Value::Array(arr);
    }
    let mut map = Map::new();
    map.insert(idx_to_string(idx), Value::Null);
    Value::Object(map)
}

pub(super) fn delete_object_value_by_property_path(
    root: &mut Value,
    path: &str,
) -> miette::Result<bool> {
    let segments = parse_property_path(path)
        .map_err(|err| miette::Report::new(PkgError::InvalidPropertyPath(err)))?;
    let Some((last, parents)) = segments.split_last() else {
        return Ok(false);
    };
    check_unsafe_key_in_path(path)?;
    let mut current = root;
    for segment in parents {
        let Some(next) = descend_for_delete(current, segment)? else {
            return Ok(false);
        };
        current = next;
    }
    remove_value(current, last)
}

/// Step into the container `segment` names. `None` when the document
/// has nothing there — a path that does not exist deletes nothing.
fn descend_for_delete<'a>(
    current: &'a mut Value,
    segment: &Segment,
) -> miette::Result<Option<&'a mut Value>> {
    let key = match segment {
        Segment::Key(key) => key.clone(),
        Segment::Index(idx) => {
            let index = validate_index(*idx)?;
            if current.is_array() {
                let arr = current.as_array_mut().expect("the value is an array");
                return Ok(arr.get_mut(index));
            }
            if !current.is_object() {
                return Ok(None);
            }
            idx_to_string(*idx)
        }
    };
    let Some(obj) = current.as_object_mut() else { return Ok(None) };
    Ok(obj.get_mut(&key))
}

/// Remove what the path's last segment names, reporting whether
/// anything was there.
fn remove_value(current: &mut Value, last: &Segment) -> miette::Result<bool> {
    let key = match last {
        Segment::Key(key) => key.clone(),
        Segment::Index(idx) => {
            let index = validate_index(*idx)?;
            if current.is_array() {
                let arr = current.as_array_mut().expect("the value is an array");
                if index >= arr.len() {
                    return Ok(false);
                }
                arr.remove(index);
                return Ok(true);
            }
            if !current.is_object() {
                return Ok(false);
            }
            idx_to_string(*idx)
        }
    };
    let Some(obj) = current.as_object_mut() else { return Ok(false) };
    Ok(obj.remove(&key).is_some())
}
