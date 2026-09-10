use super::{IntoDiagnostic, PublishSummary, Value};

/// One `+ <pkg> (staged...)` line of the non-JSON `stage publish` output.
pub(super) fn render_stage_publish_summary(summary: &PublishSummary, dry_run: bool) -> String {
    if dry_run {
        return format!("+ {} (would stage)", summary.id);
    }
    match &summary.stage_id {
        Some(stage_id) => format!("+ {} (staged with id {stage_id})", summary.id),
        None => format!("+ {} (staged)", summary.id),
    }
}

/// Render one staged item as `key: value` lines: the known fields in a fixed
/// order, then any extra fields the registry returned, `null`s skipped.
pub(super) fn render_stage_item(item: &Value) -> String {
    let Some(object) = item.as_object() else {
        return render_value(item);
    };
    let mut lines: Vec<String> = Vec::new();
    let mut push = |key: &str, value: Option<&Value>| {
        if let Some(value) = value.filter(|value| !value.is_null()) {
            lines.push(format!("{key}: {}", render_value(value)));
        }
    };
    push("id", object.get("id"));
    push("package name", object.get("packageName"));
    push("version", object.get("version"));
    push("tag", object.get("tag"));
    push("date staged", object.get("createdAt"));
    let staged_by = match object
        .get("actorType")
        .and_then(Value::as_str)
        .filter(|actor_type| !actor_type.is_empty())
    {
        Some(actor_type) => {
            let actor = object
                .get("actor")
                .filter(|value| !value.is_null())
                .map(render_value)
                .unwrap_or_default();
            Some(Value::String(format!("{actor} ({actor_type})")))
        }
        None => object.get("actor").cloned(),
    };
    push("staged by", staged_by.as_ref());
    push("shasum", object.get("shasum"));
    const KNOWN_KEYS: [&str; 8] =
        ["id", "packageName", "version", "tag", "createdAt", "actor", "actorType", "shasum"];
    for (key, value) in object {
        if !KNOWN_KEYS.contains(&key.as_str()) {
            push(key, Some(value));
        }
    }
    lines.join("\n")
}

/// A value on a `key: value` line: strings raw, scalars via their JSON text,
/// objects and arrays as compact JSON.
fn render_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        other => serde_json::to_string(other).expect("a JSON value serializes"),
    }
}

/// The non-JSON `stage download` report: the tarball's contents and details,
/// in pnpm's `renderTarballSummary` shape.
pub(super) fn render_tarball_summary(summary: &PublishSummary) -> String {
    let files: Vec<&str> = summary.files.iter().map(|file| file.path.as_str()).collect();
    format!(
        "package: {name}@{version}\nTarball Contents\n{contents}\nTarball Details\nname: \
         {name}\nversion: {version}\nfilename: {filename}\npackage size: {size}\nunpacked size: \
         {unpacked_size}\nshasum: {shasum}\nintegrity: {integrity}\ntotal files: {entry_count}",
        name = summary.name,
        version = summary.version,
        contents = files.join("\n"),
        filename = summary.filename,
        size = summary.size,
        unpacked_size = summary.unpacked_size,
        shasum = summary.shasum,
        integrity = summary.integrity,
        entry_count = summary.entry_count,
    )
}

/// `JSON.stringify(value, null, 2)` — the two-space-indented JSON the
/// `--json` outputs print.
pub(super) fn json_pretty(value: &Value) -> miette::Result<String> {
    serde_json::to_string_pretty(value).into_diagnostic()
}
