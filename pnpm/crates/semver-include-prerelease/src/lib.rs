//! npm's `includePrerelease` range semantics, built on `node_semver`.

use std::borrow::Cow;

use node_semver::{MAX_SAFE_INTEGER, Range, Version};

/// A semver range evaluated the way npm's `semver` does with
/// `includePrerelease: true`.
///
/// The option does two things, and only the first is what its name
/// suggests. It drops the rule that a prerelease is eligible only for
/// comparators carrying a prerelease of the same `major.minor.patch`.
/// Separately, it lowers a bound npm itself synthesized for an omitted
/// component to that version's `-0` prerelease: `^18.x` becomes
/// `>=18.0.0-0`, while the explicit `^18.0.0` stays `>=18.0.0` and goes
/// on rejecting `18.0.0-rc.1`.
///
/// [`Range::satisfies`] hardcodes the eligibility rule and exposes no
/// options, so neither half can be reached through it.
pub struct IncludePrereleaseRange {
    /// The `||`-separated alternatives. A version satisfies the range
    /// when it satisfies every comparator of any one alternative; an
    /// alternative with no comparators is npm's `*`.
    alternatives: Vec<Vec<Comparator>>,
}

impl IncludePrereleaseRange {
    /// Parses `range`, dropping comparators `node_semver` rejects the
    /// way its own parser drops unparsable simples. An alternative that
    /// is nothing but such comparators is dropped whole, so a range no
    /// parser accepts is satisfied by nothing rather than by everything.
    #[must_use]
    pub fn parse(range: &str) -> Self {
        IncludePrereleaseRange { alternatives: range.split("||").filter_map(comparators).collect() }
    }

    /// Whether `version` lies inside any one of the range's
    /// `||`-separated alternatives.
    ///
    /// A prerelease is eligible wherever its own release would be, and
    /// clears a lower bound npm synthesized for an omitted component
    /// exactly when that release clears it — the two halves of
    /// `includePrerelease` [`IncludePrereleaseRange`] describes.
    #[must_use]
    pub fn satisfies(&self, version: &Version) -> bool {
        // The eligibility rule `includePrerelease` drops only ever
        // excludes prereleases, so for a release `node_semver` already
        // answers with the endpoint test — and answers it without the
        // single-version ranges the prerelease path has to build.
        if !version.is_prerelease() {
            return self.alternatives.iter().any(|alternative| {
                alternative.iter().all(|comparator| comparator.bounds.satisfies(version))
            });
        }
        let Some(point) = point_range(version) else { return false };
        let release_point = point_range(&release_of(version));
        self.alternatives.iter().any(|alternative| {
            alternative
                .iter()
                .all(|comparator| comparator.satisfies(&point, release_point.as_ref()))
        })
    }
}

/// One comparator of an alternative, with its endpoints as `node_semver`
/// read them.
struct Comparator {
    bounds: Range,
    /// Whether npm would have lowered this comparator's lower bound to
    /// the `-0` prerelease of the version it names, which it does for a
    /// bound it synthesized rather than one the range spelled out. A
    /// prerelease clears such a bound exactly when its release does,
    /// `-0` being the lowest prerelease of any version.
    lower_bound_admits_prereleases: bool,
}

impl Comparator {
    /// Whether the version behind `point` lies inside this comparator,
    /// `release_point` being that version stripped of its prerelease tag
    /// (`None` when it carries none).
    ///
    /// Both are single-version ranges rather than versions because
    /// membership is tested through [`Range::allows_any`]: overlap with
    /// one point is the same endpoint test [`Range::satisfies`] does,
    /// minus the prerelease eligibility rule `includePrerelease` drops.
    fn satisfies(&self, point: &Range, release_point: Option<&Range>) -> bool {
        self.bounds.allows_any(point)
            || self.lower_bound_admits_prereleases
                && release_point.is_some_and(|release| self.bounds.allows_any(release))
    }
}

/// The comparators of one `||`-separated alternative, or `None` when
/// every token in a non-empty alternative was unparsable. A hyphen range
/// spans three whitespace-separated tokens; every other comparator is
/// one token, once an operator has been glued to the version it bounds.
fn comparators(alternative: &str) -> Option<Vec<Comparator>> {
    let glued = glue_operators_to_versions(alternative);
    let tokens: Vec<&str> = glued.split_whitespace().collect();
    let mut comparators = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let is_hyphen_range = tokens.get(index + 1) == Some(&"-") && index + 2 < tokens.len();
        let (text, lower_bound_admits_prereleases, width): (Cow<'_, str>, _, _) = if is_hyphen_range
        {
            let text = format!("{} - {}", tokens[index], tokens[index + 2]);
            (text.into(), !names_a_prerelease(tokens[index]), 3)
        } else {
            let token = tokens[index];
            let text = npm_upper_bound(token).map_or(Cow::Borrowed(token), Cow::Owned);
            (text, omits_a_component(token), 1)
        };
        if let Ok(bounds) = text.parse::<Range>() {
            comparators.push(Comparator { bounds, lower_bound_admits_prereleases });
        }
        index += width;
    }
    (tokens.is_empty() || !comparators.is_empty()).then_some(comparators)
}

/// Removes the whitespace an operator may carry before its version, so
/// `>= 1.2` stays one comparator instead of splitting into two tokens.
fn glue_operators_to_versions(alternative: &str) -> String {
    let mut glued = String::with_capacity(alternative.len());
    let mut chars = alternative.chars().peekable();
    while let Some(char) = chars.next() {
        glued.push(char);
        if matches!(char, '<' | '>' | '=' | '~' | '^') {
            while chars.peek().is_some_and(char::is_ascii_whitespace) {
                chars.next();
            }
        }
    }
    glued
}

/// Whether `token` names a version that leaves a component out — `18`,
/// `18.x`, `^18.0` — the case in which npm lowers the comparator's lower
/// bound. A hyphen range's lower endpoint is lowered whether or not it
/// omits anything, so it is asked [`names_a_prerelease`] instead.
fn omits_a_component(token: &str) -> bool {
    let core = version_of(token).split(['-', '+']).next().unwrap_or_default();
    let mut components = core.split('.');
    let named = [components.next(), components.next(), components.next()];
    named.into_iter().any(|component| {
        component
            .is_none_or(|component| component.is_empty() || matches!(component, "x" | "X" | "*"))
    })
}

/// npm reads an upper bound whose version omits a component as an
/// exclusive bound at the first version the comparator leaves out:
/// `<=18` is `<19.0.0-0` and `<18.1` is `<18.1.0-0`. `node_semver` reads
/// them as `<=18.0.0-0` and `<18.1.0`, both off by the omitted
/// component, so those two operators are rewritten before it sees them.
/// Returns `None` for every other token, including a fully spelled-out
/// bound, which it already agrees on, and a component past the largest
/// one `node_semver` accepts, whose rewrite it would only reject again.
fn npm_upper_bound(token: &str) -> Option<String> {
    let (inclusive, version) = match token.strip_prefix("<=") {
        Some(version) => (true, version),
        None => (false, token.strip_prefix('<')?),
    };
    let version = version.trim_start_matches('v');
    if version.contains(['-', '+']) {
        return None;
    }
    let mut components = version.split('.');
    let major: u64 = components.next()?.parse().ok()?;
    let minor: Option<u64> = components.next().and_then(|minor| minor.parse().ok());
    let patch_is_named =
        components.next().is_some_and(|patch| patch.parse::<u64>().is_ok()) && minor.is_some();
    let past_the_end = |component: u64| {
        let bound = component.checked_add(u64::from(inclusive))?;
        (bound <= MAX_SAFE_INTEGER).then_some(bound)
    };
    match (minor, patch_is_named) {
        (_, true) => None,
        (None, _) => Some(format!("<{}.0.0-0", past_the_end(major)?)),
        (Some(minor), _) => Some(format!("<{major}.{}.0-0", past_the_end(minor)?)),
    }
}

fn names_a_prerelease(token: &str) -> bool {
    version_of(token).split('+').next().unwrap_or_default().contains('-')
}

fn version_of(token: &str) -> &str {
    token.trim_start_matches(['<', '>', '=', '~', '^']).trim_start_matches('v')
}

fn release_of(version: &Version) -> Version {
    Version {
        major: version.major,
        minor: version.minor,
        patch: version.patch,
        pre_release: Vec::new(),
        build: Vec::new(),
    }
}

fn point_range(version: &Version) -> Option<Range> {
    version.to_string().parse().ok()
}

#[cfg(test)]
mod tests;
