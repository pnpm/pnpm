use super::{
    Display, EnvVar, Error, LoadWorkspaceYamlError, Path, Placeholder, WorkspaceSettings,
    drop_placeholders, env_replace_lossy, placeholder_ranges, resolvable_placeholders,
    resolve_placeholders,
};
use std::ops::Range;

/// Error encountered when parsing settings from a configuration file.
#[derive(Debug, Display, Error)]
pub(crate) enum ParseSettingsError {
    Yaml(Box<serde_saphyr::Error>),
    #[display("Failed to replace env in config: {var}")]
    UnresolvedEnvVar {
        var: String,
    },
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
///
/// Only the placeholders the document cannot be read without are resolved
/// here. A setting that takes free text reads as written, so its placeholder
/// is left for the trusted / untrusted substitution that follows — which is
/// what keeps a repository-controlled file from naming a request destination
/// out of the environment. A document that reads as written resolves nothing
/// at all and is returned exactly as it was read, so what a file means today
/// it goes on meaning.
///
/// A document that cannot be read because a setting is written as a
/// placeholder with no value and no fallback reports that placeholder.
pub(crate) fn parse_settings<Sys: EnvVar>(
    text: &str,
) -> Result<WorkspaceSettings, ParseSettingsError> {
    parse_resolving_placeholders::<Sys>(text)
        .map_err(|error| match first_unreadable_unresolved_placeholder::<Sys>(text) {
            Some(var) => ParseSettingsError::UnresolvedEnvVar { var },
            None => ParseSettingsError::Yaml(error),
        })
}

/// [`parse_settings`] for the file at `path`.
pub(crate) fn parse_settings_file<Sys: EnvVar>(
    text: &str,
    path: &Path,
) -> Result<WorkspaceSettings, LoadWorkspaceYamlError> {
    parse_settings::<Sys>(text)
        .map_err(|error| match error {
            ParseSettingsError::Yaml(source) => {
                LoadWorkspaceYamlError::ParseYaml { path: path.to_path_buf(), source }
            }
            ParseSettingsError::UnresolvedEnvVar { var } => {
                LoadWorkspaceYamlError::ConfigUnresolvedEnvVar { var }
            }
        })
}

/// The first placeholder with no value and no fallback that the document
/// cannot be read with, if the document reads once those are all dropped.
///
/// The search halves the candidates on each read, so it costs about one read
/// per doubling of their number, however many the file holds.
fn first_unreadable_unresolved_placeholder<Sys: EnvVar>(text: &str) -> Option<String> {
    let unresolved = unresolved_placeholders::<Sys>(text);
    let resolvable = resolvable_placeholders::<Sys>(text);
    let reads_keeping = |kept: Range<usize>| {
        let replaced = replaced_placeholders(&resolvable, &unresolved, kept);
        read_text(&resolve_placeholders(text, &replaced, |_| true)).is_some()
    };
    if unresolved.is_empty() || !reads_keeping(0..0) {
        return None;
    }
    let mut candidates = 0..unresolved.len();
    while candidates.len() > 1 {
        let middle = candidates.start + candidates.len() / 2;
        candidates = if reads_keeping(candidates.start..middle) {
            middle..candidates.end
        } else {
            candidates.start..middle
        };
    }
    let (_, var) = &unresolved[candidates.start];
    (!reads_keeping(candidates)).then(|| var.clone())
}

/// The placeholders of `text` with no value and no fallback, each with the
/// spelling an error reports it by.
///
/// A placeholder that fills a quoted scalar takes the quotes into its range,
/// so dropping it leaves a null rather than the string `"null"`.
fn unresolved_placeholders<Sys: EnvVar>(text: &str) -> Vec<(Range<usize>, String)> {
    placeholder_ranges(text)
        .into_iter()
        .filter_map(|range| {
            let (_, unresolved) = env_replace_lossy::<Sys>(&text[range.clone()]);
            unresolved
                .into_iter()
                .next()
                .map(|var| (with_enclosing_quotes(text, range), var))
        })
        .collect()
}

fn with_enclosing_quotes(text: &str, range: Range<usize>) -> Range<usize> {
    let bytes = text.as_bytes();
    match (
        range.start
            .checked_sub(1)
            .map(|before| bytes[before]),
        bytes.get(range.end),
    ) {
        (Some(open @ (b'"' | b'\'')), Some(&close)) if open == close => {
            range.start - 1..range.end + 1
        }
        _ => range,
    }
}

/// Every resolvable placeholder resolved, and every unresolved one outside
/// `kept` replaced by `null`, in document order.
fn replaced_placeholders(
    resolvable: &[Placeholder],
    unresolved: &[(Range<usize>, String)],
    kept: Range<usize>,
) -> Vec<Placeholder> {
    let mut replaced = resolvable.to_vec();
    replaced.extend(
        unresolved
            .iter()
            .enumerate()
            .filter(|(index, _)| !kept.contains(index))
            .map(|(_, (range, _))| Placeholder { range: range.clone(), resolved: "null".into() }),
    );
    replaced.sort_by_key(|placeholder| placeholder.range.start);
    replaced
}

/// Read `text`, resolving the placeholders it cannot be read without.
fn parse_resolving_placeholders<Sys: EnvVar>(
    text: &str,
) -> Result<WorkspaceSettings, Box<serde_saphyr::Error>> {
    // A placeholder reaches serde as its own text, which only a string-valued
    // setting can hold, so a document carrying one for any other setting has
    // to be read a second time with that placeholder already resolved.
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
    // Resolving a placeholder the document reads without would take a setting
    // out of the hands of the substitution that knows which layer the file
    // came from, so each one that turns out not to be needed is put back.
    if !decide(text, &placeholders, &mut resolved, 0..placeholders.len(), &mut reads) {
        return Err(Box::new(as_written));
    }
    read_text(&resolve_placeholders(text, &placeholders, |index| resolved[index]))
        .ok_or_else(|| Box::new(as_written))
}

/// Work out which of `group` the document cannot be read without, leaving
/// `resolved` false for the rest, and report whether it stayed inside
/// [`MAX_DOCUMENT_READS`].
///
/// Resolving one placeholder says nothing about whether another is needed, so
/// a group put back all at once answers for every placeholder in it: the
/// document reads when none of them was needed, and otherwise the group is
/// halved.
fn decide(
    text: &str,
    placeholders: &[Placeholder],
    resolved: &mut [bool],
    group: Range<usize>,
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

/// The error a document that will not read even with its placeholders
/// resolved reports.
///
/// The document read without them answers which it is. It reads when a
/// placeholder is what the reader stumbled over, and the message must not
/// repeat what one resolved to. Otherwise the file says something it cannot
/// mean on its own, and that read is the one that says what: the first read
/// stops at the placeholder, which is not the problem and whose line would
/// send a reader to the wrong setting.
fn classify(text: &str, placeholders: &[Placeholder]) -> serde_saphyr::Error {
    match serde_saphyr::from_str::<WorkspaceSettings>(&drop_placeholders(text, placeholders)) {
        Ok(_) => serde::de::Error::custom(INVALID_EXPANSION),
        Err(dropped) => dropped,
    }
}

fn read_text(text: &str) -> Option<WorkspaceSettings> {
    serde_saphyr::from_str(text).ok()
}
