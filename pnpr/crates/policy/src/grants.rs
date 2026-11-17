//! Grants: what a subject may do to the packages of one registry.

use super::{AccessList, AccessToken, Identity, PackageRule};
use pnpr_registry::{Ecosystem, PackagePattern};

/// What a grant lets a subject do to a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// Read metadata and files. On an upstream: what the cache holds.
    Read,
    /// On an upstream: fetch what the cache lacks.
    Cache,
    /// Publish a version, push an image, upload a file.
    Publish,
    /// Change dist-tags, deprecations, notes, maintainers, and yanks.
    Annotate,
    /// Unpublish, and delete an image or a file.
    Delete,
    /// Change the subjects of the grant that gives it.
    Manage,
}

impl Action {
    pub const ALL: [Self; 6] =
        [Self::Read, Self::Cache, Self::Publish, Self::Annotate, Self::Delete, Self::Manage];

    /// The action as a grant spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Cache => "cache",
            Self::Publish => "publish",
            Self::Annotate => "annotate",
            Self::Delete => "delete",
            Self::Manage => "manage",
        }
    }

    #[must_use]
    pub fn parse(action: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == action)
    }

    /// Whether a package's owners can be given the action through `$owners`.
    #[must_use]
    pub const fn suits_owners(self) -> bool {
        matches!(self, Self::Publish | Self::Annotate | Self::Delete)
    }
}

/// A grant as it applies to one registry, before its patterns are parsed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GrantSpec {
    pub name: String,
    /// The patterns of the packages the grant covers; none is every package.
    pub include: Vec<String>,
    /// The patterns of the packages taken out of it.
    pub exclude: Vec<String>,
    /// Each subject, as an access list entry spells it, with one action.
    pub subjects: Vec<(String, Action)>,
}

/// A [`GrantSpec`] whose patterns are parsed for the registry's ecosystem.
#[derive(Debug, Clone)]
pub struct Grant {
    name: String,
    include: Vec<PackagePattern>,
    exclude: Vec<PackagePattern>,
    subjects: Vec<(AccessToken, Action)>,
}

impl Grant {
    /// Parse `spec` for a registry of `ecosystem`. The error is the reason
    /// only.
    pub fn compile(spec: &GrantSpec, ecosystem: Ecosystem) -> Result<Self, String> {
        let parse = |patterns: &[String]| {
            patterns
                .iter()
                .map(|pattern| {
                    PackagePattern::parse(pattern, ecosystem).map_err(|error| error.to_string())
                })
                .collect::<Result<Vec<_>, _>>()
        };
        let subjects = spec.subjects
            .iter()
            .map(|(subject, action)| {
                let token = AccessToken::parse(subject);
                if token == AccessToken::Owners && !action.suits_owners() {
                    return Err(format!("`$owners` cannot be given `{}`", action.as_str()));
                }
                Ok((token, *action))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            name: spec.name.clone(),
            include: parse(&spec.include)?,
            exclude: parse(&spec.exclude)?,
            subjects,
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The grant as a [`GrantSpec`] spells it.
    #[must_use]
    pub fn spec(&self) -> GrantSpec {
        let patterns = |patterns: &[PackagePattern]| {
            patterns
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        GrantSpec {
            name: self.name.clone(),
            include: patterns(&self.include),
            exclude: patterns(&self.exclude),
            subjects: self.subjects
                .iter()
                .map(|(token, action)| (token.to_string(), *action))
                .collect(),
        }
    }

    /// Whether the grant covers `package`.
    #[must_use]
    pub fn covers(&self, package: &str) -> bool {
        (self.include.is_empty()
            || self.include
                .iter()
                .any(|pattern| pattern.matches(package)))
            && !self.exclude
                .iter()
                .any(|pattern| pattern.matches(package))
    }

    /// Whether the grant gives `action` to a subject `identity` matches.
    #[must_use]
    pub fn gives(&self, action: Action, identity: &Identity, is_owner: bool) -> bool {
        self.subjects
            .iter()
            .any(|(token, given)| *given == action && identity.matches(token, is_owner))
    }

    fn gives_owners(&self, action: Action) -> bool {
        self.subjects
            .iter()
            .any(|(token, given)| *given == action && *token == AccessToken::Owners)
    }

    pub(super) fn names_team(&self, team: &str) -> bool {
        self.subjects
            .iter()
            .any(|(token, _)| matches!(token, AccessToken::Team(id) if id == team))
    }

    pub(super) fn refines_read(&self) -> bool {
        let gives_read = self.subjects
            .iter()
            .any(|(_, action)| *action == Action::Read);
        gives_read && !(self.include.is_empty() && self.exclude.is_empty())
    }

    /// Whether the grant narrows who may read `package` to less than the
    /// registry at large: it gives `read` and selects packages by pattern.
    fn refines_read_of(&self, package: &str) -> bool {
        let gives_read = self.subjects
            .iter()
            .any(|(_, action)| *action == Action::Read);
        gives_read
            && (self.exclude
                .iter()
                .any(|pattern| pattern.matches(package))
                || (!self.include.is_empty() && self.covers(package)))
    }
}

/// The grants that cover one registry.
#[derive(Debug, Default, Clone)]
pub(super) struct Grants(pub(super) Vec<Grant>);

impl Grants {
    pub(super) fn permits(
        &self,
        package: &str,
        action: Action,
        identity: &Identity,
        is_owner: bool,
    ) -> bool {
        self.0
            .iter()
            .any(|grant| grant.covers(package) && grant.gives(action, identity, is_owner))
    }

    pub(super) fn owners_may(&self, package: &str, action: Action) -> bool {
        self.0
            .iter()
            .any(|grant| grant.covers(package) && grant.gives_owners(action))
    }

    pub(super) fn any_admits(&self, action: Action, identity: &Identity) -> bool {
        self.0
            .iter()
            .any(|grant| grant.gives(action, identity, false))
    }

    /// Whether `identity` may do `action` on every package.
    pub(super) fn all_admit(&self, action: Action, identity: &Identity) -> bool {
        let giving: Vec<&Grant> = self.0
            .iter()
            .filter(|grant| grant.gives(action, identity, false))
            .collect();
        covered(&giving, &PackagePattern::All)
    }

    pub(super) fn refine_read_of(&self, package: &str) -> bool {
        self.0
            .iter()
            .any(|grant| grant.refines_read_of(package))
    }
}

/// Whether `grants` between them cover every package `pattern` matches. A
/// grant covers the pattern less the patterns it excludes inside it, which
/// the grants must cover in turn. Two patterns are nested or disjoint, and an
/// excluded one is more specific than the one it is inside, so this ends.
fn covered(grants: &[&Grant], pattern: &PackagePattern) -> bool {
    grants
        .iter()
        .any(|grant| {
            let included = grant.include.is_empty()
                || grant.include
                    .iter()
                    .any(|include| include.covers(pattern));
            included
                && grant.exclude
                    .iter()
                    .all(|exclude| {
                        !exclude.covers(pattern)
                            && (!pattern.covers(exclude) || covered(grants, exclude))
                    })
        })
}

/// The lists a rule shorthand falls back to.
pub(super) struct Defaults<'lists> {
    pub(super) access: &'lists AccessList,
    pub(super) publish: &'lists AccessList,
    pub(super) unpublish: &'lists AccessList,
}

/// The subjects of the grant for `rule`, or for the packages no rule names:
/// who may read gets `read` and `cache`, who may publish `publish` and
/// `annotate`, who may unpublish `delete`.
fn subjects_of(rule: Option<&PackageRule>, defaults: &Defaults<'_>) -> Vec<(AccessToken, Action)> {
    let access = rule.and_then(|rule| rule.access.as_ref()).unwrap_or(defaults.access);
    let publish = rule.and_then(|rule| rule.publish.as_ref()).unwrap_or(defaults.publish);
    let unpublish = rule.and_then(|rule| rule.unpublish.as_ref()).unwrap_or(defaults.unpublish);
    let given = [
        (access, &[Action::Read, Action::Cache][..]),
        (publish, &[Action::Publish, Action::Annotate][..]),
        (unpublish, &[Action::Delete][..]),
    ];
    let mut subjects = Vec::new();
    for (list, actions) in given {
        for token in list.tokens() {
            subjects.extend(
                actions
                    .iter()
                    .map(|action| (token.clone(), *action)),
            );
        }
    }
    subjects
}

/// The grants that give what `rules` and `defaults` give under
/// most-specific-rule-wins: one grant for each rule, over its pattern less
/// the patterns of the more specific rules it covers, and one for every
/// package no rule names.
pub(super) fn grants_of_rules(rules: &[PackageRule], defaults: &Defaults<'_>) -> Vec<Grant> {
    let mut grants: Vec<Grant> = rules
        .iter()
        .map(|rule| Grant {
            name: rule.pattern.to_string(),
            include: vec![rule.pattern.clone()],
            exclude: rules
                .iter()
                .filter(|other| {
                    other.pattern != rule.pattern && rule.pattern.covers(&other.pattern)
                })
                .map(|other| other.pattern.clone())
                .collect(),
            subjects: subjects_of(Some(rule), defaults),
        })
        .collect();
    grants.push(Grant {
        name: String::new(),
        include: Vec::new(),
        exclude: rules
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect(),
        subjects: subjects_of(None, defaults),
    });
    grants
}
