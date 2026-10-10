use miette::IntoDiagnostic;
use serde_json::Value;
use std::fmt::Write as _;

pub(super) const KNOWN_PROFILE_KEYS: &[&str] = &[
    "name",
    "email",
    "two-factor auth",
    "fullname",
    "homepage",
    "freenode",
    "twitter",
    "github",
    "created",
    "updated",
];

pub(super) fn render_single_property(
    cleaned: &[(String, String)],
    prop: &str,
    parseable: bool,
) -> String {
    let val = cleaned
        .iter()
        .find(|(key, _)| key == prop)
        .map_or("", |(_, value)| value.as_str());
    if parseable { format!("{prop}\t{val}") } else { val.to_string() }
}

pub(super) fn render_all_properties(
    cleaned: &[(String, String)],
    parseable: bool,
) -> miette::Result<String> {
    let mut out = String::new();
    let separator = if parseable { '\t' } else { ':' };
    for (key, val) in cleaned {
        if parseable {
            writeln!(&mut out, "{key}{separator}{val}").into_diagnostic()?;
        } else {
            writeln!(&mut out, "{key}{separator} {val}").into_diagnostic()?;
        }
    }
    Ok(out.trim_end().to_string())
}

pub(crate) fn format_profile_map(info: &Value) -> Vec<(String, String)> {
    let mut map = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for key in KNOWN_PROFILE_KEYS {
        seen.insert(*key);
        map.push(((*key).to_string(), format_profile_field(key, info)));
    }
    append_extra_profile_fields(&mut map, info, &seen);
    map
}

fn format_profile_field(key: &str, info: &Value) -> String {
    match key {
        "email" => format_email_field(info),
        "two-factor auth" => format_tfa_field(info),
        other => match info.get(other) {
            Some(v) if v.is_null() => String::new(),
            Some(v) => v.as_str().map_or_else(|| v.to_string(), ToString::to_string),
            None => String::new(),
        },
    }
}

fn format_email_field(info: &Value) -> String {
    let email = info
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or("");
    let verified = info
        .get("email_verified")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if email.is_empty() {
        String::new()
    } else if verified {
        format!("{email} (verified)")
    } else {
        format!("{email} (unverified)")
    }
}

fn format_tfa_field(info: &Value) -> String {
    match info.get("tfa") {
        Some(tfa) => tfa
            .get("mode")
            .and_then(Value::as_str)
            .map_or_else(|| "disabled".to_string(), ToString::to_string),
        None => "disabled".to_string(),
    }
}

fn append_extra_profile_fields(
    map: &mut Vec<(String, String)>,
    info: &Value,
    seen: &std::collections::HashSet<&str>,
) {
    let Some(obj) = info.as_object() else { return };
    for (k, v) in obj {
        if seen.contains(k.as_str()) || k == "tfa" || k == "email_verified" || k == "cidr_whitelist"
        {
            continue;
        }
        let val_str = match v {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        map.push((k.clone(), val_str));
    }
}
