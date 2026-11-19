use super::{
    ActionReference, Component, Document, HashSet, Path, PathBuf, QueryError, Range, Route,
    VecDeque, fs, parse_comment_version,
};

mod root;
mod routes;

pub(super) async fn discover(root: &Path) -> miette::Result<Vec<ActionReference>> {
    let root_display = root.display();
    let canonical_root = fs::canonicalize(root).await
        .map_err(|error| miette::miette!("Failed to read {root_display}: {error}"))?;
    let mut queue = workflow_files(&root.join(".github/workflows")).await?;
    let mut visited = HashSet::new();
    let mut actions = Vec::new();
    while let Some(file) = queue.pop_front() {
        let file_display = file.display();
        let real_file = fs::canonicalize(&file).await
            .map_err(|error| miette::miette!("Failed to read {file_display}: {error}"))?;
        if !real_file.starts_with(&canonical_root) {
            return Err(miette::miette!(
                code = "ERR_PNPM_GITHUB_ACTIONS_WORKFLOW_OUTSIDE_ROOT",
                "GitHub Actions workflow is outside the project root: {file_display}"
            ));
        }
        if !visited.insert(real_file.clone()) {
            continue;
        }
        let scan = scan_workflow_file(root, &canonical_root, &real_file).await?;
        queue.extend(scan.local_references);
        actions.extend(scan.actions);
    }
    Ok(actions)
}

/// The workflow files in `workflows`. A project with no workflow directory
/// has none.
async fn workflow_files(workflows: &Path) -> miette::Result<VecDeque<PathBuf>> {
    let workflows_display = workflows.display();
    let mut entries = match fs::read_dir(workflows).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(VecDeque::new()),
        Err(error) => {
            return Err(miette::miette!("Failed to read {workflows_display}: {error}"));
        }
    };
    let mut queue = VecDeque::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| miette::miette!("Failed to read {workflows_display}: {error}"))?
    {
        let path = entry.path();
        if matches!(path.extension().and_then(|ext| ext.to_str()), Some("yml" | "yaml")) {
            queue.push_back(path);
        }
    }
    Ok(queue)
}

/// What one workflow file's `uses:` values yield: the actions it pins, and
/// the local workflows and composite actions it pulls in, which are
/// scanned in turn.
struct WorkflowScan {
    actions: Vec<ActionReference>,
    local_references: Vec<PathBuf>,
}

async fn scan_workflow_file(
    root: &Path,
    canonical_root: &Path,
    real_file: &Path,
) -> miette::Result<WorkflowScan> {
    let real_file_display = real_file.display();
    let text = fs::read_to_string(real_file).await
        .map_err(|error| miette::miette!("Failed to read {real_file_display}: {error}"))?;
    let mut scan = WorkflowScan { actions: Vec::new(), local_references: Vec::new() };
    for uses_value in uses_values(&text)
        .map_err(|error| miette::miette!("Failed to parse {real_file_display}: {error}"))?
    {
        let (value, comment) = split_uses_value(uses_value.value);
        if let Some(local) = value
            .strip_prefix("./")
            .or_else(|| value.strip_prefix("$/"))
        {
            if let Some(candidate) = resolve_local_reference(root, canonical_root, local).await? {
                scan.local_references.push(candidate);
            }
            continue;
        }
        if let Some(action) = action_reference(uses_value, value, comment, real_file) {
            scan.actions.push(action);
        }
    }
    Ok(scan)
}

/// The action a `uses:` value pins, or `None` when it names a docker image
/// or is not spelled `owner/repository@ref`.
fn action_reference(
    uses_value: UsesValue<'_>,
    value: &str,
    comment: Option<&str>,
    real_file: &Path,
) -> Option<ActionReference> {
    let (name, ref_and_comment) = value.rsplit_once('@')?;
    if name.starts_with("docker://") {
        return None;
    }
    let mut parts = name.split('/');
    let (Some(owner), Some(repository)) = (parts.next(), parts.next()) else {
        return None;
    };
    let comment_version = comment_version_token(comment);
    Some(ActionReference {
        file: real_file.to_path_buf(),
        name: name.to_string(),
        ref_: ref_and_comment.to_string(),
        repo: format!("{owner}/{repository}"),
        source: crate::WorkflowValue {
            comment_version,
            flow_style: uses_value.flow_style,
            indentation: uses_value.indentation,
            original_value: uses_value.value.to_string(),
            range: uses_value.range,
        },
    })
}

async fn resolve_local_reference(
    root: &Path,
    canonical_root: &Path,
    reference: &str,
) -> miette::Result<Option<PathBuf>> {
    let target = root.join(reference);
    let candidate = if matches!(
        target.extension().and_then(|extension| extension.to_str()),
        Some("yml" | "yaml"),
    ) {
        existing_file(&target).await?.then_some(target)
    } else {
        let action_yml = target.join("action.yml");
        if existing_file(&action_yml).await? {
            Some(action_yml)
        } else {
            let action_yaml = target.join("action.yaml");
            existing_file(&action_yaml).await?.then_some(action_yaml)
        }
    };
    let Some(candidate) = candidate else { return Ok(None) };
    let candidate_display = candidate.display();
    let candidate = fs::canonicalize(&candidate).await
        .map_err(|error| miette::miette!("Failed to read {candidate_display}: {error}"))?;
    Ok(candidate.starts_with(canonical_root).then_some(candidate))
}

async fn existing_file(path: &Path) -> miette::Result<bool> {
    match fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            let path_display = path.display();
            Err(miette::miette!("Failed to read {path_display}: {error}"))
        }
    }
}

pub(super) fn comment_version_token(comment: Option<&str>) -> Option<String> {
    comment
        .and_then(|comment| comment.split_whitespace().next())
        .filter(|candidate| parse_comment_version(candidate).is_some())
        .map(str::to_string)
}

pub(super) fn split_uses_value(value: &str) -> (&str, Option<&str>) {
    let value = value.trim();
    if let Some(quote) = value
        .chars()
        .next()
        .filter(|quote| matches!(quote, '\'' | '"'))
        && let Some(end) = value[1..].find(quote)
    {
        let end = end + 1;
        let comment = value[end + quote.len_utf8()..]
            .trim()
            .strip_prefix('#')
            .map(str::trim);
        return (&value[1..end], comment);
    }
    match unquoted_comment_offset(value) {
        Some(offset) => (value[..offset].trim_end(), Some(value[offset + 1..].trim())),
        None => (value, None),
    }
}

fn unquoted_comment_offset(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    bytes
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, &byte)| {
            (byte == b'#' && bytes[index - 1].is_ascii_whitespace()).then_some(index)
        })
}

struct UsesValue<'a> {
    flow_style: bool,
    indentation: String,
    range: Range<usize>,
    value: &'a str,
}

fn uses_values(text: &str) -> Result<Vec<UsesValue<'_>>, QueryError> {
    let document = Document::new(text)?;
    let mut seen_ranges = HashSet::new();
    let mut values = Vec::new();
    for route in routes::uses_routes(text, &document)? {
        if let Some(uses) = uses_value_at(text, &document, &route)?
            && seen_ranges.insert(uses.range.clone())
        {
            values.push(uses);
        }
    }
    Ok(values)
}

/// The `uses:` scalar at `route` with its byte range in `text`, or `None`
/// when the route names no plain `key: value` scalar.
fn uses_value_at<'text>(
    text: &'text str,
    document: &Document,
    route: &Route<'_>,
) -> Result<Option<UsesValue<'text>>, QueryError> {
    let Some(feature) = (match document.query_exact(route) {
        Ok(feature) => feature,
        Err(QueryError::ExhaustedMapping(_) | QueryError::UnexpectedNode(_)) => return Ok(None),
        Err(error) => return Err(error),
    }) else {
        return Ok(None);
    };
    let key = match document.query_key_only(route) {
        Ok(key) => key,
        Err(QueryError::ExhaustedMapping(_) | QueryError::UnexpectedNode(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let (start, scalar_end) = feature.location.byte_span;
    let (Some(before_start), Some(raw_scalar), Some(end_info)) =
        (text.get(..start), text.get(start..scalar_end), uses_scalar_end(text, scalar_end))
    else {
        return Ok(None);
    };
    let line_start = before_start
        .rfind('\n')
        .map(|line_break| line_break + 1)
        .unwrap_or(0);
    if !is_inline_uses_target(
        text,
        key.location.byte_span.1,
        start,
        raw_scalar,
        &before_start[line_start..],
    ) {
        return Ok(None);
    }
    let Some(value) = text.get(start..end_info.end) else {
        return Ok(None);
    };
    Ok(Some(UsesValue {
        flow_style: end_info.flow_style,
        indentation: " ".repeat(start - line_start),
        range: start..end_info.end,
        value,
    }))
}

fn is_inline_uses_target(
    text: &str,
    key_end: usize,
    start: usize,
    raw_scalar: &str,
    line_prefix: &str,
) -> bool {
    if raw_scalar.contains('\n') || raw_scalar.starts_with(['|', '>']) {
        return false;
    }
    text.get(key_end..start)
        .is_some_and(is_inline_uses_separator)
        || has_trailing_anchor(line_prefix)
}

fn is_inline_uses_separator(separator: &str) -> bool {
    let Some(after_colon) = separator.strip_prefix(':') else {
        return false;
    };
    after_colon.chars().all(char::is_whitespace)
        || (after_colon.trim_start().starts_with('&') && has_trailing_anchor(after_colon))
}

fn has_trailing_anchor(prefix: &str) -> bool {
    let Some((_, after_amp)) = prefix.rsplit_once('&') else {
        return false;
    };
    let anchor_len = after_amp
        .find(|character: char| {
            character.is_whitespace() || matches!(character, '[' | ']' | '{' | '}' | ',')
        })
        .unwrap_or(0);
    anchor_len > 0 && after_amp[anchor_len..].chars().all(char::is_whitespace)
}

struct UsesScalarEnd {
    end: usize,
    flow_style: bool,
}

fn uses_scalar_end(text: &str, scalar_end: usize) -> Option<UsesScalarEnd> {
    let after_scalar = text.get(scalar_end..)?;
    let line_end = after_scalar
        .find('\n')
        .map(|end| scalar_end + end)
        .unwrap_or(text.len());
    let trailing = text.get(scalar_end..line_end)?;
    let following = trailing.trim_start();
    let flow_style = matches!(following.chars().next(), Some('}' | ']' | ','));
    let end = if following.starts_with('#') {
        scalar_end + trailing.trim_end().len()
    } else if flow_style {
        scalar_end + trailing.len() - following.len()
    } else {
        scalar_end
    };
    Some(UsesScalarEnd { end, flow_style })
}
