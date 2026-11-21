use super::{
    ActionReference,
    HashSet,
    Path,
    PathBuf,
    QueryError,
    Range,
    VecDeque,
    fs,
    parse_comment_version,
};

mod graph;
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
        if let Some(local) = uses_value.decoded
            .strip_prefix("./")
            .or_else(|| uses_value.decoded.strip_prefix("$/"))
        {
            if let Some(candidate) = resolve_local_reference(root, canonical_root, local).await? {
                scan.local_references.push(candidate);
            }
            continue;
        }
        if let Some(action) = action_reference(uses_value, real_file) {
            scan.actions.push(action);
        }
    }
    Ok(scan)
}

/// The action a `uses:` value pins, or `None` when it names a docker image
/// or is not spelled `owner/repository@ref`.
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::needless_borrowed_parameters,
        reason = "copied only when the `uses:` value names an action; one path serves the whole file"
    )
)]
pub(super) fn action_reference(
    uses_value: UsesValue<'_>,
    real_file: &Path,
) -> Option<ActionReference> {
    if uses_value.decoded.chars().any(char::is_control) {
        return None;
    }
    let (name, ref_) = uses_value.decoded.rsplit_once('@')?;
    if ref_.is_empty() || name.starts_with("docker://") {
        return None;
    }
    let mut parts = name.split('/');
    let (Some(owner), Some(repository)) = (parts.next(), parts.next()) else {
        return None;
    };
    if owner.is_empty() || repository.is_empty() {
        return None;
    }
    let comment_version = comment_version_token(uses_value.trailing_comment());
    Some(ActionReference {
        file: real_file.to_path_buf(),
        name: name.to_string(),
        ref_: ref_.to_string(),
        repo: format!("{owner}/{repository}"),
        source: crate::WorkflowValue {
            comment_version,
            flow_style: uses_value.flow_style,
            indentation: uses_value.indentation,
            original_value: uses_value.value.to_string(),
            range: uses_value.range,
            scalar_len: uses_value.scalar_len,
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

fn comment_version_token(comment: Option<&str>) -> Option<String> {
    comment
        .map(str::split_whitespace)
        .and_then(|mut words| words.next())
        .filter(|candidate| parse_comment_version(candidate).is_some())
        .map(str::to_string)
}

pub(super) struct UsesValue<'a> {
    decoded: String,
    flow_style: bool,
    indentation: String,
    range: Range<usize>,
    scalar_len: usize,
    value: &'a str,
}

impl<'a> UsesValue<'a> {
    fn trailing_comment(&self) -> Option<&'a str> {
        self.value
            .get(self.scalar_len..)?
            .trim_start()
            .strip_prefix('#')
            .map(str::trim)
    }
}

pub(super) fn uses_values(text: &str) -> Result<Vec<UsesValue<'_>>, QueryError> {
    let mut seen_ranges = HashSet::new();
    let mut values = Vec::new();
    for scalar in routes::uses_scalars(text)? {
        if let Some(uses) = uses_value_at(text, scalar)
            && seen_ranges.insert(uses.range.clone())
        {
            values.push(uses);
        }
    }
    Ok(values)
}

/// The single-line `uses:` scalar at `scalar.span` with any trailing same-line
/// comment included in its byte range.
fn uses_value_at(text: &str, scalar: routes::UsesScalar) -> Option<UsesValue<'_>> {
    let before_start = text.get(..scalar.span.start)?;
    let end_info = uses_scalar_end(text, scalar.span.end)?;
    let value = text.get(scalar.span.start..end_info.end)?;
    let scalar_len = scalar.span.end.checked_sub(scalar.span.start)?;
    let line_start = before_start
        .rfind('\n')
        .map(|line_break| line_break + 1)
        .unwrap_or(0);
    Some(UsesValue {
        decoded: scalar.decoded,
        flow_style: end_info.flow_style,
        indentation: " ".repeat(scalar.span.start - line_start),
        range: scalar.span.start..end_info.end,
        scalar_len,
        value,
    })
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
