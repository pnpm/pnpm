use super::{DocumentPlaceholders, EnvVar, Placeholder, document_placeholders};
use restore::deserialize_restoring;
use serde::de::DeserializeOwned;

mod restore;

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

#[derive(Clone, Copy)]
enum PlaceholderChoice {
    Sentinel,
    Expanded,
}

/// Read the settings of a `pnpm-workspace.yaml` / `config.yaml`, resolving a
/// setting written as `${VAR}` or `${VAR:-fallback}`.
///
/// Only the placeholders the document cannot be read without are resolved
/// here. Unresolved placeholders stand in as YAML-safe bare-token sentinels
/// during parsing and are restored to their literal `${...}` text as serde
/// reads string values, so flow-style collections and fallback values with `: `
/// still leave free-text settings unexpanded for the trusted / untrusted
/// substitution that follows. A document that reads as written resolves
/// nothing at all and is returned exactly as it was read.
pub(crate) fn parse_settings<Sys: EnvVar, Settings: DeserializeOwned + Default>(
    text: &str,
) -> Result<Settings, Box<serde_saphyr::Error>> {
    let as_written = match serde_saphyr::from_str::<Option<Settings>>(text) {
        Ok(Some(settings)) => return Ok(settings),
        Ok(None) => return Ok(Settings::default()),
        Err(error) => error,
    };
    let Some(placeholders) = document_placeholders::<Sys>(text) else {
        return Err(Box::new(as_written));
    };
    let mut choices: Vec<PlaceholderChoice> = placeholders.items
        .iter()
        .map(|placeholder| {
            if placeholder.resolved.is_some() {
                PlaceholderChoice::Expanded
            } else {
                PlaceholderChoice::Sentinel
            }
        })
        .collect();
    let Some(initial) = read_candidate::<Settings>(text, &placeholders, &choices) else {
        if placeholders.resolvable.is_empty() {
            return Err(Box::new(as_written));
        }
        return Err(Box::new(classify::<Settings>(text, &placeholders, as_written)));
    };
    if placeholders.resolvable.is_empty() {
        return Ok(initial);
    }
    let mut reads = 1;
    if !decide::<Settings>(text, &placeholders, &mut choices, &placeholders.resolvable, &mut reads)
    {
        return Err(Box::new(as_written));
    }
    read_candidate(text, &placeholders, &choices).ok_or_else(|| Box::new(as_written))
}

/// Work out which indices in `group` the document cannot be read without,
/// leaving `choices` set to [`PlaceholderChoice::Sentinel`] for the rest, and
/// report whether it stayed inside [`MAX_DOCUMENT_READS`].
fn decide<Settings: DeserializeOwned + Default>(
    text: &str,
    placeholders: &DocumentPlaceholders,
    choices: &mut [PlaceholderChoice],
    group: &[usize],
    reads: &mut u32,
) -> bool {
    if *reads >= MAX_DOCUMENT_READS {
        return false;
    }
    *reads += 1;
    set_group(choices, group, PlaceholderChoice::Sentinel);
    if read_candidate::<Settings>(text, placeholders, choices).is_some() {
        return true;
    }
    set_group(choices, group, PlaceholderChoice::Expanded);
    if group.len() == 1 {
        return true;
    }
    let (left, right) = group.split_at(group.len() / 2);
    decide::<Settings>(text, placeholders, choices, left, reads)
        && decide::<Settings>(text, placeholders, choices, right, reads)
}

fn set_group(choices: &mut [PlaceholderChoice], group: &[usize], choice: PlaceholderChoice) {
    for &index in group {
        choices[index] = choice;
    }
}

/// Report the error for a document that will not read even with its resolvable
/// placeholders expanded, redacting expanded values while keeping source
/// locations when another setting is at fault.
fn classify<Settings: DeserializeOwned + Default>(
    text: &str,
    placeholders: &DocumentPlaceholders,
    as_written: serde_saphyr::Error,
) -> serde_saphyr::Error {
    let dropped_with_sentinels = placeholders.render(text, |_, placeholder| {
        if placeholder.resolved.is_some() { "null" } else { &placeholder.sentinel }
    });
    if deserialize_restoring::<Settings>(&dropped_with_sentinels, text, placeholders).is_some() {
        return serde::de::Error::custom(INVALID_EXPANSION);
    }
    let dropped_as_written = placeholders.render(text, |_, placeholder| {
        if placeholder.resolved.is_some() { "null" } else { &text[placeholder.range.clone()] }
    });
    match serde_saphyr::from_str::<Option<Settings>>(&dropped_as_written) {
        Ok(_) => as_written,
        Err(dropped) => dropped,
    }
}

fn read_candidate<Settings: DeserializeOwned + Default>(
    text: &str,
    placeholders: &DocumentPlaceholders,
    choices: &[PlaceholderChoice],
) -> Option<Settings> {
    let candidate = placeholders.render(text, |index, placeholder: &Placeholder| {
        match (choices[index], placeholder.resolved.as_deref()) {
            (PlaceholderChoice::Expanded, Some(value)) => value,
            (PlaceholderChoice::Expanded, None) | (PlaceholderChoice::Sentinel, _) => {
                &placeholder.sentinel
            }
        }
    });
    deserialize_restoring(&candidate, text, placeholders)
}
