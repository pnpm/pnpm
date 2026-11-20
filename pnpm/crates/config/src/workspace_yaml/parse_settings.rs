use super::{
    DocumentPlaceholders,
    EnvVar,
    LoadWorkspaceYamlError,
    Path,
    Placeholder,
    document_placeholders,
    env_replace_lossy,
};
use restore::deserialize_restoring;
use serde::de::DeserializeOwned;
use std::ops::Range;

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlaceholderChoice {
    Sentinel,
    Expanded,
}

struct Prober<'text> {
    text: &'text str,
    placeholders: &'text DocumentPlaceholders,
    choices: Vec<PlaceholderChoice>,
    reads: u32,
}

impl<'text> Prober<'text> {
    fn new(text: &'text str, placeholders: &'text DocumentPlaceholders) -> Self {
        let choices = placeholders.items
            .iter()
            .map(|placeholder| {
                if placeholder.resolved.is_some() {
                    PlaceholderChoice::Expanded
                } else {
                    PlaceholderChoice::Sentinel
                }
            })
            .collect();
        Self { text, placeholders, choices, reads: 0 }
    }

    fn read<Settings: DeserializeOwned + Default>(&self) -> Option<Settings> {
        read_candidate(self.text, self.placeholders, &self.choices)
    }

    /// Demote as many indices in `group` as possible from `Expanded` to
    /// `Sentinel` while `self.read::<Settings>()` continues to succeed,
    /// returning `false` if [`MAX_DOCUMENT_READS`] is exceeded.
    fn demote<Settings: DeserializeOwned + Default>(&mut self, group: &[usize]) -> bool {
        if group.is_empty() {
            return true;
        }
        if self.reads >= MAX_DOCUMENT_READS {
            return false;
        }
        self.reads += 1;
        self.set_group(group, PlaceholderChoice::Sentinel);
        if self.read::<Settings>().is_some() {
            return true;
        }
        self.set_group(group, PlaceholderChoice::Expanded);
        if group.len() == 1 {
            return true;
        }
        let (left, right) = group.split_at(group.len() / 2);
        self.demote::<Settings>(left) && self.demote::<Settings>(right)
    }

    /// Confirm every placeholder left `Expanded` is rejected even as an
    /// unrestored sentinel by `serde_saphyr`, ruling out free-text slots whose
    /// restoration failed on a quote or duplicate-key check.
    fn verify_expanded_require_resolution<Settings: DeserializeOwned + Default>(&mut self) -> bool {
        for index in 0..self.choices.len() {
            if self.choices[index] != PlaceholderChoice::Expanded {
                continue;
            }
            if self.reads >= MAX_DOCUMENT_READS {
                return false;
            }
            self.reads += 1;
            self.choices[index] = PlaceholderChoice::Sentinel;
            let candidate = render_choices(self.text, self.placeholders, &self.choices);
            self.choices[index] = PlaceholderChoice::Expanded;
            if serde_saphyr::from_str::<Option<Settings>>(&candidate).is_ok() {
                return false;
            }
        }
        true
    }

    fn set_group(&mut self, group: &[usize], choice: PlaceholderChoice) {
        for &index in group {
            self.choices[index] = choice;
        }
    }
}

/// Read the settings of a `pnpm-workspace.yaml` / `config.yaml`, resolving a
/// setting written as `${VAR}` or `${VAR:-fallback}`.
///
/// Only the placeholders the document cannot be read without are resolved
/// here. Placeholders whose YAML syntax indicators (`{`, `}`, `: `) would
/// break parsing stand in as style-probing sentinels and are restored to their
/// decoded `${...}` text as serde reads string values. Free-text settings are
/// therefore always left unexpanded for the trusted / untrusted substitution
/// that follows. A document that reads as written resolves nothing at all.
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
    let mut prober = Prober::new(text, &placeholders);
    prober.reads = 1;
    if prober.read::<Settings>().is_none() {
        if placeholders.resolvable.is_empty() {
            return Err(Box::new(as_written));
        }
        return Err(Box::new(classify::<Settings>(text, &placeholders, as_written)));
    }
    if !prober.demote::<Settings>(&placeholders.resolvable)
        || !prober.verify_expanded_require_resolution::<Settings>()
    {
        return Err(Box::new(as_written));
    }
    prober.read().ok_or_else(|| Box::new(as_written))
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

fn render_choices(
    text: &str,
    placeholders: &DocumentPlaceholders,
    choices: &[PlaceholderChoice],
) -> String {
    placeholders.render(text, |index, placeholder: &Placeholder| {
        match (choices[index], placeholder.resolved.as_deref()) {
            (PlaceholderChoice::Expanded, Some(value)) => value,
            (PlaceholderChoice::Expanded, None) | (PlaceholderChoice::Sentinel, _) => {
                &placeholder.sentinel
            }
        }
    })
}

fn read_candidate<Settings: DeserializeOwned + Default>(
    text: &str,
    placeholders: &DocumentPlaceholders,
    choices: &[PlaceholderChoice],
) -> Option<Settings> {
    let candidate = render_choices(text, placeholders, choices);
    deserialize_restoring(&candidate, text, placeholders)
}

/// [`parse_settings`] for the file at `path`.
///
/// A document that cannot be read because a setting is written as a
/// placeholder with no value and no fallback reports that placeholder.
pub(crate) fn parse_settings_file<Sys: EnvVar, Settings: DeserializeOwned + Default>(
    text: &str,
    path: &Path,
) -> Result<Settings, LoadWorkspaceYamlError> {
    parse_settings::<Sys, Settings>(text)
        .map_err(|source| match first_unreadable_unresolved_placeholder::<Sys, Settings>(text) {
            Some(var) => LoadWorkspaceYamlError::ConfigUnresolvedEnvVar { var },
            None => LoadWorkspaceYamlError::ParseYaml { path: path.to_path_buf(), source },
        })
}

/// The first placeholder with no value and no fallback that the document
/// cannot be read with, if the document reads once those are all dropped.
///
/// The search halves the candidates on each read, so it costs about one read
/// per doubling of their number, however many the file holds.
fn first_unreadable_unresolved_placeholder<Sys: EnvVar, Settings: DeserializeOwned + Default>(
    text: &str,
) -> Option<String> {
    let placeholders = document_placeholders::<Sys>(text)?;
    let unresolved: Vec<(usize, String)> = placeholders.items
        .iter()
        .enumerate()
        .filter(|(_, placeholder)| placeholder.in_value)
        .filter_map(|(index, placeholder)| {
            let (_, unresolved) = env_replace_lossy::<Sys>(&text[placeholder.range.clone()]);
            unresolved
                .into_iter()
                .next()
                .map(|var| (index, var))
        })
        .collect();
    let reads_keeping = |kept: Range<usize>| {
        let mut dropped = vec![false; placeholders.items.len()];
        for (position, (index, _)) in unresolved.iter().enumerate() {
            dropped[*index] = !kept.contains(&position);
        }
        let candidate = render_dropping(text, &placeholders, &dropped);
        deserialize_restoring::<Settings>(&candidate, text, &placeholders).is_some()
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

/// Render `text` with each `dropped` placeholder replaced by `null`, every
/// resolvable one expanded, and the rest as their sentinels.
///
/// A dropped placeholder that fills a quoted scalar takes the quotes with it,
/// so it leaves a null rather than the string `"null"`. A quote another
/// dropped placeholder already took stays with that one.
fn render_dropping(text: &str, placeholders: &DocumentPlaceholders, dropped: &[bool]) -> String {
    let mut rendered = String::with_capacity(text.len());
    let mut copied = 0;
    for (placeholder, &dropped) in placeholders.items.iter().zip(dropped) {
        if dropped {
            let range = with_enclosing_quotes(text, placeholder.range.clone(), copied);
            rendered.push_str(&text[copied..range.start]);
            rendered.push_str("null");
            copied = range.end;
        } else {
            rendered.push_str(&text[copied..placeholder.range.start]);
            rendered.push_str(placeholder.resolved.as_deref().unwrap_or(&placeholder.sentinel));
            copied = placeholder.range.end;
        }
    }
    rendered.push_str(&text[copied..]);
    rendered
}

fn with_enclosing_quotes(text: &str, range: Range<usize>, copied: usize) -> Range<usize> {
    let bytes = text.as_bytes();
    match (
        range.start
            .checked_sub(1)
            .filter(|&before| before >= copied)
            .map(|before| bytes[before]),
        bytes.get(range.end),
    ) {
        (Some(open @ (b'"' | b'\'')), Some(&close)) if open == close => {
            range.start - 1..range.end + 1
        }
        _ => range,
    }
}
