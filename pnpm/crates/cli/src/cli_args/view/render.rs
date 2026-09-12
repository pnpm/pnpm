use super::{DateTime, Map, OwoColorize, Stream, Style, Utc, Value};

/// Render the selected `fields` of `info`. A single field unwraps to its
/// value (raw for `--json`, plain for text); multiple fields render as a
/// `{field: value}` object (`--json`) or `field = value` lines.
pub(super) fn render_fields(info: &Value, fields: &[String], json: bool) -> String {
    let selected: Vec<(&String, Option<Value>)> =
        fields.iter().map(|field| (field, get_nested_property(info, field))).collect();

    if json {
        if let [(_, value)] = selected.as_slice() {
            return value.as_ref().map(to_pretty).unwrap_or_default();
        }
        let map: Map<String, Value> = selected
            .iter()
            .filter_map(|(field, value)| value.clone().map(|value| ((*field).clone(), value)))
            .collect();
        return to_pretty(&Value::Object(map));
    }

    if let [(_, value)] = selected.as_slice() {
        return format_field_value(value.as_ref());
    }

    selected
        .iter()
        .map(|(field, value)| match value {
            Some(value @ (Value::Object(_) | Value::Array(_))) => {
                format!("{field} = {}", serde_json::to_string(value).unwrap_or_default())
            }
            Some(Value::String(string)) => format!("{field} = '{string}'"),
            other => format!("{field} = {}", format_field_value(other.as_ref())),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Walk a dotted `path` (e.g. `dist.shasum`) through `info`, returning a
/// clone of the value or `None` when any segment is missing or not an
/// object.
pub(super) fn get_nested_property(info: &Value, path: &str) -> Option<Value> {
    let mut current = info;
    for part in path.split('.') {
        current = current.as_object()?.get(part)?;
    }
    Some(current.clone())
}

/// Stringify a single field value for text output: `null`/absent is empty,
/// objects and arrays are pretty JSON, strings pass through, and scalars use
/// their plain form.
pub(super) fn format_field_value(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(value @ (Value::Object(_) | Value::Array(_))) => to_pretty(value),
        Some(Value::String(string)) => string.clone(),
        Some(other) => other.to_string(),
    }
}

/// Render the human-readable summary block: a `name@version | license |
/// deps | versions` header followed by description, homepage, deprecation,
/// keywords, bin, dist, dependencies, maintainers, dist-tags, and the
/// published-by line.
pub(super) fn render_summary(info: &Value) -> String {
    let mut lines: Vec<String> = vec![summary_header(info)];

    if let Some(description) = str_field(info, "description") {
        lines.push(description.to_string());
    }
    if let Some(homepage) = str_field(info, "homepage") {
        lines.push(underline_blue(homepage));
    }
    if let Some(deprecated) = str_field(info, "deprecated") {
        lines.push(String::new());
        lines.push(format!("{} - {deprecated}", red("DEPRECATED!")));
    }
    if let Some(keywords) = array_field(info, "keywords") {
        let joined = keywords.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
        lines.push(String::new());
        lines.push(format!("keywords: {}", cyan(&joined)));
    }

    lines.extend(bin_summary(info));
    lines.extend(dist_lines(info));
    lines.extend(dependencies_lines(info));
    lines.extend(maintainers_lines(info));
    lines.extend(dist_tags_lines(info));

    if let Some(published) = published_info(info) {
        lines.push(String::new());
        lines.push(published);
    }

    lines.join("\n")
}

/// The single line naming the package, its license, and its counts.
fn summary_header(info: &Value) -> String {
    let mut header: Vec<String> = Vec::new();
    if let (Some(name), Some(version)) = (str_field(info, "name"), str_field(info, "version")) {
        header.push(cyan(&format!("{name}@{version}")));
    }
    if let Some(license) = str_field(info, "license") {
        header.push(green(license));
    }
    match info.get("depsCount").and_then(Value::as_u64) {
        Some(count) => header.push(format!("deps: {}", cyan(&count.to_string()))),
        None => header.push("deps: none".to_string()),
    }
    if let Some(count) = info.get("versionsCount").and_then(Value::as_u64) {
        header.push(format!("versions: {}", cyan(&count.to_string())));
    }
    header.join(" | ")
}

fn dist_lines(info: &Value) -> Vec<String> {
    let Some(dist) = info.get("dist").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut lines = vec![String::new(), bold("dist")];
    if let Some(tarball) = obj_str(dist, "tarball") {
        lines.push(format!(".tarball: {}", underline_blue(tarball)));
    }
    if let Some(shasum) = obj_str(dist, "shasum") {
        lines.push(format!(".shasum: {}", green(shasum)));
    }
    if let Some(integrity) = obj_str(dist, "integrity") {
        lines.push(format!(".integrity: {}", green(integrity)));
    }
    if let Some(unpacked_size) = dist.get("unpackedSize").and_then(Value::as_u64) {
        lines.push(format!(".unpackedSize: {}", blue(&format_bytes(unpacked_size))));
    }
    lines
}

fn dependencies_lines(info: &Value) -> Vec<String> {
    let Some(dependencies) = info.get("dependencies").and_then(Value::as_object) else {
        return Vec::new();
    };
    if dependencies.is_empty() {
        return Vec::new();
    }
    let entries: Vec<String> = dependencies
        .iter()
        .map(|(name, version)| format!("{}: {}", blue(name), version.as_str().unwrap_or_default()))
        .collect();
    vec![String::new(), "dependencies:".to_string(), entries.join(", ")]
}

fn maintainers_lines(info: &Value) -> Vec<String> {
    let Some(maintainers) = array_field(info, "maintainers") else {
        return Vec::new();
    };
    let mut lines = vec![String::new(), "maintainers:".to_string()];
    for maintainer in maintainers {
        lines.push(format!("- {}", format_person(maintainer)));
    }
    lines
}

fn dist_tags_lines(info: &Value) -> Vec<String> {
    let Some(dist_tags) = info.get("distTags").and_then(Value::as_object) else {
        return Vec::new();
    };
    if dist_tags.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![String::new(), bold("dist-tags:")];
    for (tag, version) in dist_tags {
        lines.push(format!("{}: {}", blue(tag), version.as_str().unwrap_or_default()));
    }
    lines
}

/// Render the `bin:` summary line(s). A string `bin` derives its single
/// command from the (scope-stripped) package name; an object `bin` lists
/// its keys.
pub(super) fn bin_summary(info: &Value) -> Vec<String> {
    let bins: Vec<String> = match info.get("bin") {
        Some(Value::String(bin)) if !bin.is_empty() => match str_field(info, "name") {
            Some(name) if name.starts_with('@') => {
                vec![name.split_once('/').map_or(name, |(_, rest)| rest).to_string()]
            }
            Some(name) => vec![name.to_string()],
            None => Vec::new(),
        },
        Some(Value::Object(bin)) => bin.keys().cloned().collect(),
        _ => Vec::new(),
    };
    if bins.is_empty() {
        return Vec::new();
    }
    vec![String::new(), format!("bin: {}", cyan(&bins.join(", ")))]
}

/// Build the `published <time> ago[ by <publisher>]` line. Needs the picked
/// version's publish timestamp; an unparsable timestamp yields no line, and
/// a future one degrades to "just now".
pub(super) fn published_info(info: &Value) -> Option<String> {
    let version = str_field(info, "version")?;
    let published_time = info.get("time")?.as_object()?.get(version)?.as_str()?;
    let date = parse_date(published_time)?;
    let time_ago = format_time_ago(date).unwrap_or_else(|| "just now".to_string());
    Some(match publisher(info) {
        Some(publisher) => format!("published {} by {publisher}", cyan(&time_ago)),
        None => format!("published {}", cyan(&time_ago)),
    })
}

/// Resolve the publisher shown in the published-by line, preferring
/// `_npmUser`, then the first maintainer (with `et al.` when there are
/// more), then the author.
pub(super) fn publisher(info: &Value) -> Option<String> {
    if let Some(npm_user) = info.get("_npmUser").and_then(Value::as_object)
        && obj_str(npm_user, "name").is_some()
    {
        return Some(format_person(&Value::Object(npm_user.clone())));
    }
    if let Some(maintainers) = array_field(info, "maintainers") {
        let formatted = format_person(&maintainers[0]);
        return Some(if maintainers.len() == 1 {
            formatted
        } else {
            format!("{formatted} et al.")
        });
    }
    str_field(info, "author").map(ToString::to_string)
}

/// Format a `{ name, email }` person object as `name <email>` (blue name,
/// dimmed email), or just the name when no email is present.
pub(super) fn format_person(person: &Value) -> String {
    let name = person.get("name").and_then(Value::as_str).unwrap_or("");
    match person.get("email").and_then(Value::as_str).filter(|email| !email.is_empty()) {
        Some(email) => format!("{} <{}>", blue(name), dim(email)),
        None => blue(name),
    }
}

/// Format a byte count with a 1000-based unit and at most two decimals.
pub(super) fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }
    const SIZES: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];
    let bytes = bytes as f64;
    let index = bytes.log(1000_f64).floor() as usize;
    let index = index.min(SIZES.len() - 1);
    let value = (bytes / 1000_f64.powi(index as i32) * 100.0).round() / 100.0;
    format!("{value} {}", SIZES[index])
}

/// The age of `date` relative to `now`, bucketed into a coarse "N unit(s)
/// ago" label. `None` for a future date (clock skew). Split from
/// [`format_time_ago`] so the `now` reference is injectable in tests.
pub(super) fn format_time_ago_since(date: DateTime<Utc>, now: DateTime<Utc>) -> Option<String> {
    let diff_ms = now.signed_duration_since(date).num_milliseconds();
    if diff_ms < 0 {
        return None;
    }
    let diff_sec = diff_ms / 1000;
    let diff_min = diff_sec / 60;
    let diff_hour = diff_min / 60;
    let diff_day = diff_hour / 24;
    let diff_month = diff_day / 30;
    let diff_year = diff_day / 365;
    let unit = |count: i64, singular: &str| {
        format!("{count} {singular}{} ago", if count == 1 { "" } else { "s" })
    };
    Some(if diff_year > 0 {
        unit(diff_year, "year")
    } else if diff_month > 0 {
        unit(diff_month, "month")
    } else if diff_day > 0 {
        unit(diff_day, "day")
    } else if diff_hour > 0 {
        unit(diff_hour, "hour")
    } else if diff_min > 0 {
        unit(diff_min, "minute")
    } else {
        "a few seconds ago".to_string()
    })
}

fn format_time_ago(date: DateTime<Utc>) -> Option<String> {
    format_time_ago_since(date, Utc::now())
}

pub(super) fn parse_date(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|date| date.with_timezone(&Utc))
}

pub(super) fn to_pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// A field's string value, treating an empty string as absent.
fn str_field<'a>(info: &'a Value, key: &str) -> Option<&'a str> {
    info.get(key).and_then(Value::as_str).filter(|value| !value.is_empty())
}

fn obj_str<'a>(map: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    map.get(key).and_then(Value::as_str).filter(|value| !value.is_empty())
}

/// A field's array value, treating an empty array as absent.
fn array_field<'a>(info: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    info.get(key).and_then(Value::as_array).filter(|array| !array.is_empty())
}

fn cyan(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, OwoColorize::cyan).to_string()
}

fn green(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, OwoColorize::green).to_string()
}

fn blue(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, OwoColorize::blue).to_string()
}

fn red(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, OwoColorize::red).to_string()
}

fn bold(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, OwoColorize::bold).to_string()
}

fn dim(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, OwoColorize::dimmed).to_string()
}

fn underline_blue(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |text| text.style(Style::new().blue().underline()))
        .to_string()
}
