//! SCIM 2.0 (RFC 7643, RFC 7644) message shapes for the `User` resource.

use super::{super::RegistryError, directory::ScimUser};
use serde_json::{Map, Value, json};

pub(super) const USER_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
const LIST_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:ListResponse";
const PATCH_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";

/// The most keys a PATCH path may name: a schema URN, an attribute, and a
/// sub-attribute, as in `urn:...:User:manager.value`.
const MAX_PATH_DEPTH: usize = 3;

/// Attributes pnpr derives itself or never stores.
const MANAGED_ATTRIBUTES: [&str; 6] = ["schemas", "id", "meta", "userName", "active", "password"];

/// A user as the SCIM API returns it.
pub(super) fn user_resource(username: &str, user: &ScimUser, location: &str) -> Value {
    let mut resource = user.attributes.clone();
    resource.insert("schemas".to_string(), json!([USER_SCHEMA]));
    resource.insert("id".to_string(), json!(username));
    resource.insert("userName".to_string(), json!(username));
    resource.insert("active".to_string(), json!(user.active));
    resource.insert("meta".to_string(), json!({ "resourceType": "User", "location": location }));
    Value::Object(resource)
}

pub(super) fn list_response(resources: &[Value], total: usize, start_index: usize) -> Value {
    json!({
        "schemas": [LIST_SCHEMA],
        "totalResults": total,
        "startIndex": start_index,
        "itemsPerPage": resources.len(),
        "Resources": resources,
    })
}

/// The `userName` and the user a `POST` or `PUT` body describes. `active`
/// defaults to `true`.
pub(super) fn parse_user(body: &Value) -> Result<(String, ScimUser), RegistryError> {
    let object = body.as_object().ok_or_else(|| invalid("the body must be a JSON object"))?;
    let username = object
        .get("userName")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| invalid("userName is required"))?;
    let active = object
        .get("active")
        .map_or(Ok(true), parse_active)?;
    let attributes = object
        .iter()
        .filter(|(key, _)| !MANAGED_ATTRIBUTES.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Ok((username.to_string(), ScimUser { active, attributes, ..ScimUser::default() }))
}

/// Apply a `PatchOp` body to `user`. `active` and top-level or `name.*`
/// attributes change. Operations on a filtered path such as
/// `emails[type eq "work"].value` are accepted and ignored, because pnpr
/// reads no attribute but `active`.
pub(super) fn apply_patch(user: &mut ScimUser, body: &Value) -> Result<(), RegistryError> {
    let schemas = body.get("schemas").and_then(Value::as_array);
    if !schemas.is_some_and(|schemas| {
        schemas
            .iter()
            .any(|schema| schema == PATCH_SCHEMA)
    }) {
        return Err(invalid("a PATCH body must use the PatchOp schema"));
    }
    let operations = body
        .get("Operations")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("Operations is required"))?;
    for operation in operations {
        apply_operation(user, operation)?;
    }
    Ok(())
}

fn apply_operation(user: &mut ScimUser, operation: &Value) -> Result<(), RegistryError> {
    let op = operation
        .get("op")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| invalid("every operation needs an op"))?;
    let value = operation.get("value");
    match (op.as_str(), operation.get("path").and_then(Value::as_str)) {
        ("add" | "replace", None) => {
            let values = value
                .and_then(Value::as_object)
                .ok_or_else(|| invalid("an operation without a path needs an object value"))?;
            values
                .iter()
                .try_for_each(|(path, value)| set(user, path, Some(value)))
        }
        ("add" | "replace", Some(path)) => {
            set(user, path, Some(value.ok_or_else(|| invalid("add and replace need a value"))?))
        }
        ("remove", Some(path)) => set(user, path, None),
        _ => Err(invalid(&format!("unsupported PATCH operation {op:?}"))),
    }
}

/// Set or, with `None`, remove the attribute at `path`.
fn set(user: &mut ScimUser, path: &str, value: Option<&Value>) -> Result<(), RegistryError> {
    let path = path
        .strip_prefix(USER_SCHEMA)
        .map_or(path, |rest| rest.trim_start_matches(':'));
    if path.contains('[') {
        return Ok(());
    }
    if path.eq_ignore_ascii_case("active") {
        user.active = value.map_or(Ok(false), parse_active)?;
        return Ok(());
    }
    if MANAGED_ATTRIBUTES
        .iter()
        .any(|managed| managed.eq_ignore_ascii_case(path))
    {
        return Err(RegistryError::BadRequest {
            reason: format!("{path} cannot be changed through PATCH"),
        });
    }
    let keys = attribute_keys(path);
    if keys.len() > MAX_PATH_DEPTH {
        return Err(RegistryError::BadRequest {
            reason: format!("PATCH path {path:?} nests deeper than {MAX_PATH_DEPTH} levels"),
        });
    }
    set_nested(&mut user.attributes, &keys, value);
    Ok(())
}

/// The keys of `path` in a stored resource. An extension attribute such as
/// `urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:manager.value`
/// lives under its schema URN, which itself contains dots.
fn attribute_keys(path: &str) -> Vec<&str> {
    let (urn, attribute) = match path.rsplit_once(':') {
        // A whole schema URN, as an operation without a path names it.
        Some((_, "User")) => return vec![path],
        Some((urn, attribute)) if path.starts_with("urn:") => (Some(urn), attribute),
        _ => (None, path),
    };
    urn.into_iter()
        .chain(attribute.split('.'))
        .collect()
}

/// Set or, with `None`, remove the value under `keys`, creating the objects
/// on the way.
fn set_nested(attributes: &mut Map<String, Value>, keys: &[&str], value: Option<&Value>) {
    let [key, rest @ ..] = keys else { return };
    if rest.is_empty() {
        match value {
            Some(value) => attributes.insert((*key).to_string(), value.clone()),
            None => attributes.remove(*key),
        };
        return;
    }
    let entry = attributes
        .entry((*key).to_string())
        .or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    if let Some(object) = entry.as_object_mut() {
        set_nested(object, rest, value);
    }
}

/// SCIM `active`, which some clients send as the string `"True"` or
/// `"False"`.
fn parse_active(value: &Value) -> Result<bool, RegistryError> {
    match value {
        Value::Bool(active) => Ok(*active),
        Value::String(text) if text.eq_ignore_ascii_case("true") => Ok(true),
        Value::String(text) if text.eq_ignore_ascii_case("false") => Ok(false),
        _ => Err(invalid("active must be a boolean")),
    }
}

/// The username a `filter` asks for. Only `userName eq "<name>"` is
/// supported, which is what identity providers send to find an account.
pub(super) fn parse_filter(filter: &str) -> Result<String, RegistryError> {
    let mut parts = filter.trim().splitn(3, ' ');
    let attribute = parts.next().unwrap_or_default();
    let operator = parts.next().unwrap_or_default();
    let value = parts.next().unwrap_or_default().trim();
    let quoted = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'));
    match quoted {
        Some(name)
            if attribute.eq_ignore_ascii_case("userName")
                && operator.eq_ignore_ascii_case("eq") =>
        {
            Ok(name.replace(r#"\""#, r#"""#))
        }
        _ => Err(RegistryError::BadRequest {
            reason: format!(r#"unsupported filter {filter:?}; pnpr supports userName eq "<name>""#),
        }),
    }
}

fn invalid(reason: &str) -> RegistryError {
    RegistryError::BadRequest { reason: reason.to_string() }
}
