use super::{
    Diagnostic, Display, EnvVar, Error, LoadWorkspaceYamlError, PathBuf, Placeholder,
    WorkspaceSettings, drop_placeholders, env_replace_lossy, placeholder_ranges,
    resolvable_placeholders, resolve_placeholders,
};

/// Error encountered when parsing settings from a configuration file.
#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum ParseSettingsError {
    Yaml(Box<serde_saphyr::Error>),
    #[display("Failed to replace env in config: {var}")]
    #[diagnostic(code(ERR_PNPM_CONFIG_UNRESOLVED_ENV_VAR))]
    UnresolvedEnvVar {
        var: String,
    },
}

impl ParseSettingsError {
    pub(crate) fn into_load_error(self, path: PathBuf) -> LoadWorkspaceYamlError {
        match self {
            Self::Yaml(source) => LoadWorkspaceYamlError::ParseYaml { path, source },
            Self::UnresolvedEnvVar { var } => {
                LoadWorkspaceYamlError::ConfigUnresolvedEnvVar { var }
            }
        }
    }
}

/// What a failed read reports in place of a value that came from the
/// environment. `nodeLinker: ${NPM_TOKEN}` written by mistake must not put
/// the token in a build log.
const INVALID_EXPANSION: &str = "invalid environment-expanded value";

/// How many times the second read may read the document.
///
/// Working out which placeholders are needed costs reads, and the file says
/// how many placeholders there are, so without a bound a repository could set
/// how long loading its own configuration takes.
///
/// Placeholders are decided in groups, which is what makes this affordable:
/// the ones a document reads without — in comments, and in every setting that
/// takes free text — cost about one read per doubling of their number rather
/// than one each, and only the ones a setting genuinely needs cost about two
/// apiece. A file naming this many settings out of the environment is far past
/// anything written by hand, and is read as written.
const MAX_DOCUMENT_READS: u32 = 512;

/// Read the settings of a `pnpm-workspace.yaml` / `config.yaml`, resolving a
/// setting written as `${VAR}` or `${VAR:-fallback}`.
pub(crate) fn parse_settings<Sys: EnvVar>(
    text: &str,
) -> Result<WorkspaceSettings, ParseSettingsError> {
    match parse_settings_internal::<Sys>(text) {
        Ok(settings) => Ok(settings),
        Err(yaml_err) => {
            if let Some(var) = first_unresolved_placeholder::<Sys>(text) {
                Err(ParseSettingsError::UnresolvedEnvVar { var })
            } else {
                Err(ParseSettingsError::Yaml(yaml_err))
            }
        }
    }
}

fn unresolved_in_text<Sys: EnvVar>(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    placeholder_ranges(text)
        .into_iter()
        .filter_map(|range| {
            let raw = &text[range.clone()];
            let (_, unresolved) = env_replace_lossy::<Sys>(raw);
            unresolved
                .into_iter()
                .next()
                .map(|var| (range, var))
        })
        .collect()
}

fn dropped_placeholders(
    resolvable: &[Placeholder],
    unresolved: &[(std::ops::Range<usize>, String)],
    skip_unresolved_idx: Option<usize>,
) -> Vec<Placeholder> {
    let mut dropped: Vec<Placeholder> = resolvable.to_vec();
    for (i, (range, _)) in unresolved.iter().enumerate() {
        if skip_unresolved_idx == Some(i) {
            continue;
        }
        dropped.push(Placeholder { range: range.clone(), resolved: "null".to_string() });
    }
    dropped.sort_by_key(|p| p.range.start);
    dropped
}

fn first_unresolved_placeholder<Sys: EnvVar>(text: &str) -> Option<String> {
    let unresolved = unresolved_in_text::<Sys>(text);
    if unresolved.is_empty() {
        return None;
    }
    let resolvable = resolvable_placeholders::<Sys>(text);
    let all_dropped = dropped_placeholders(&resolvable, &unresolved, None);
    read_text(&resolve_placeholders(text, &all_dropped, |_| true))?;

    for (idx, (_, var)) in unresolved.iter().enumerate() {
        let candidate = dropped_placeholders(&resolvable, &unresolved, Some(idx));
        if read_text(&resolve_placeholders(text, &candidate, |_| true)).is_none() {
            return Some(var.clone());
        }
    }
    None
}

fn parse_settings_internal<Sys: EnvVar>(
    text: &str,
) -> Result<WorkspaceSettings, Box<serde_saphyr::Error>> {
    let as_written = match serde_saphyr::from_str::<WorkspaceSettings>(text) {
        Ok(settings) => return Ok(settings),
        Err(error) => error,
    };
    let placeholders = resolvable_placeholders::<Sys>(text);
    if placeholders.is_empty() {
        return Err(Box::new(as_written));
    }
    let mut resolved = vec![true; placeholders.len()];
    let mut reads = 1;
    if read_text(&resolve_placeholders(text, &placeholders, |index| resolved[index])).is_none() {
        return Err(Box::new(classify(text, &placeholders)));
    }
    if !decide(text, &placeholders, &mut resolved, 0..placeholders.len(), &mut reads) {
        return Err(Box::new(as_written));
    }
    read_text(&resolve_placeholders(text, &placeholders, |index| resolved[index]))
        .ok_or_else(|| Box::new(as_written))
}

fn decide(
    text: &str,
    placeholders: &[Placeholder],
    resolved: &mut [bool],
    group: std::ops::Range<usize>,
    reads: &mut u32,
) -> bool {
    if *reads >= MAX_DOCUMENT_READS {
        return false;
    }
    *reads += 1;
    resolved[group.clone()].fill(false);
    if read_text(&resolve_placeholders(text, placeholders, |index| resolved[index])).is_some() {
        return true;
    }
    resolved[group.clone()].fill(true);
    if group.len() == 1 {
        return true;
    }
    let middle = group.start + group.len() / 2;
    decide(text, placeholders, resolved, group.start..middle, reads)
        && decide(text, placeholders, resolved, middle..group.end, reads)
}

fn classify(text: &str, placeholders: &[Placeholder]) -> serde_saphyr::Error {
    match serde_saphyr::from_str::<WorkspaceSettings>(&drop_placeholders(text, placeholders)) {
        Ok(_) => serde::de::Error::custom(INVALID_EXPANSION),
        Err(dropped) => dropped,
    }
}

fn read_text(text: &str) -> Option<WorkspaceSettings> {
    serde_saphyr::from_str(text).ok()
}
