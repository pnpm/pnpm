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
