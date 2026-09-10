use super::{
    ActionReference, Component, Document, HashSet, Path, PathBuf, QueryError, Range, Route, Value,
    VecDeque, fs, parse_version,
};

pub(super) async fn discover(root: &Path) -> miette::Result<Vec<ActionReference>> {
    let root_display = root.display();
    let canonical_root = fs::canonicalize(root)
        .await
        .map_err(|error| miette::miette!("Failed to read {root_display}: {error}"))?;
    let mut queue = workflow_files(&root.join(".github/workflows")).await?;
    let mut visited = HashSet::new();
    let mut actions = Vec::new();
    while let Some(file) = queue.pop_front() {
        let file_display = file.display();
        let real_file = fs::canonicalize(&file)
            .await
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
    let text = fs::read_to_string(real_file)
        .await
        .map_err(|error| miette::miette!("Failed to read {real_file_display}: {error}"))?;
    let mut scan = WorkflowScan { actions: Vec::new(), local_references: Vec::new() };
    for uses_value in uses_values(&text)
        .map_err(|error| miette::miette!("Failed to parse {real_file_display}: {error}"))?
    {
        let (value, comment) = split_uses_value(uses_value.value);
        if let Some(local) = value.strip_prefix("./").or_else(|| value.strip_prefix("$/")) {
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
    let comment_version = comment
        .and_then(|comment| comment.split_whitespace().next())
        .filter(|candidate| parse_version(candidate).is_some())
        .map(str::to_string);
    Some(ActionReference {
        comment_version,
        file: real_file.to_path_buf(),
        flow_style: uses_value.flow_style,
        indentation: uses_value.indentation,
        name: name.to_string(),
        original_value: uses_value.value.to_string(),
        range: uses_value.range,
        ref_: ref_and_comment.to_string(),
        repo: format!("{owner}/{repository}"),
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
    let candidate = fs::canonicalize(&candidate)
        .await
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

pub(super) fn split_uses_value(value: &str) -> (&str, Option<&str>) {
    let value = value.trim();
    if let Some(quote) = value.chars().next().filter(|quote| matches!(quote, '\'' | '"'))
        && let Some(end) = value[1..].find(quote)
    {
        let end = end + 1;
        let comment = value[end + quote.len_utf8()..].trim().strip_prefix('#').map(str::trim);
        return (&value[1..end], comment);
    }
    value.split_once(" #").map_or((value, None), |(value, comment)| (value, Some(comment.trim())))
}

struct UsesValue<'a> {
    flow_style: bool,
    indentation: String,
    range: Range<usize>,
    value: &'a str,
}

fn uses_values(text: &str) -> Result<Vec<UsesValue<'_>>, QueryError> {
    let value = parse_workflow(text)?;
    let document = Document::new(text)?;
    let mut values = Vec::new();
    for route in uses_routes(&value) {
        if let Some(uses) = uses_value_at(text, &document, &route)? {
            values.push(uses);
        }
    }
    Ok(values)
}

fn parse_workflow(text: &str) -> Result<Value, QueryError> {
    yaml_serde::from_str::<Value>(text).map_err(|err| {
        let (line, column) = err.location().map_or((0, 0), |loc| (loc.line(), loc.column()));
        QueryError::InvalidInput(line, column)
    })
}

/// The `uses:` scalar at `route` with its byte range in `text`, or `None`
/// when the route names no plain `key: value` scalar.
fn uses_value_at<'text>(
    text: &'text str,
    document: &Document,
    route: &Route<'_>,
) -> Result<Option<UsesValue<'text>>, QueryError> {
    let Some(feature) = document.query_exact(route)? else {
        return Ok(None);
    };
    let key = document.query_key_only(route)?;
    let (start, scalar_end) = feature.location.byte_span;
    let separator =
        start.checked_sub(key.location.byte_span.1).map(|_| &text[key.location.byte_span.1..start]);
    if separator.is_none_or(|separator| {
        !separator.starts_with(':') || !separator[1..].chars().all(char::is_whitespace)
    }) {
        return Ok(None);
    }
    let line_end = text[scalar_end..].find('\n').map_or(text.len(), |end| scalar_end + end);
    let trailing = &text[scalar_end..line_end];
    let following = trailing.trim_start();
    let flow_style = matches!(following.chars().next(), Some('}' | ']' | ','));
    let end = if following.starts_with('#') {
        scalar_end + trailing.trim_end().len()
    } else if flow_style {
        scalar_end + trailing.len() - following.len()
    } else {
        scalar_end
    };
    let line_start = text[..start].rfind('\n').map_or(0, |line_break| line_break + 1);
    Ok(Some(UsesValue {
        flow_style,
        indentation: " ".repeat(start - line_start),
        range: start..end,
        value: &text[start..end],
    }))
}

fn uses_routes(value: &Value) -> Vec<Route<'static>> {
    let mut routes = Vec::new();
    if let Some(jobs) = value.get("jobs").and_then(Value::as_mapping) {
        for (name, job) in jobs {
            let Some(name) = name.as_str() else { continue };
            if job.get("uses").and_then(Value::as_str).is_some() {
                routes.push(Route::from(vec![
                    "jobs".into(),
                    name.to_string().into(),
                    "uses".into(),
                ]));
            }
            add_step_routes(
                &mut routes,
                job.get("steps"),
                &["jobs".into(), name.to_string().into(), "steps".into()],
            );
        }
    }
    add_step_routes(
        &mut routes,
        value.get("runs").and_then(|runs| runs.get("steps")),
        &["runs".into(), "steps".into()],
    );
    routes
}

fn add_step_routes(
    routes: &mut Vec<Route<'static>>,
    steps: Option<&Value>,
    prefix: &[Component<'static>],
) {
    let Some(steps) = steps.and_then(Value::as_sequence) else { return };
    for (index, step) in steps.iter().enumerate() {
        if step.get("uses").and_then(Value::as_str).is_none() {
            continue;
        }
        let mut route = prefix.to_owned();
        route.extend([index.into(), "uses".into()]);
        routes.push(Route::from(route));
    }
}
