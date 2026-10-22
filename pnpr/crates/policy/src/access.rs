use super::TeamDirectory;
use std::sync::Arc;

/// A single token in an access list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessToken {
    /// `$all` — anyone, authenticated or not.
    All,
    /// `$authenticated` — any caller carrying valid Bearer or Basic
    /// credentials.
    Authenticated,
    /// `$anonymous` — only callers *without* valid credentials.
    Anonymous,
    /// `$owners` — package owners / maintainers.
    Owners,
    /// A bare token: a username. Matches an authenticated caller whose
    /// username equals it — never a team name; teams are referenced with
    /// the explicit `team:` form.
    User(String),
    /// A `team:<name>` reference to the owning registry's roster. Matches an
    /// authenticated caller whose username is a member of `name` at the
    /// moment the token is evaluated.
    Team { name: String, directory: TeamDirectory },
}

/// Only the `$`-sigiled spellings are built-ins; any other token is a
/// username, which can only *narrow* access, so this stays infallible.
/// Near-miss spellings (verdaccio's `@all`/bare aliases, an unknown
/// `$...`) and `team:` references are handled at YAML load (`AccessSpec`
/// in the config module): the former are rejected, the latter resolved
/// against the registry's declared teams. A programmatic caller passing
/// one of those spellings just gets a username that matches nobody.
impl From<&str> for AccessToken {
    fn from(token: &str) -> Self {
        match token {
            "$all" => AccessToken::All,
            "$authenticated" => AccessToken::Authenticated,
            "$anonymous" => AccessToken::Anonymous,
            "$owners" => AccessToken::Owners,
            name => AccessToken::User(name.to_string()),
        }
    }
}

/// One `access` / `publish` permission: the set of tokens that satisfy
/// it. An empty list admits no one (an explicit `unpublish: []`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AccessList(Vec<AccessToken>);

impl AccessList {
    /// Build from already-resolved tokens.
    #[must_use]
    pub fn new(tokens: Vec<AccessToken>) -> Self {
        Self(tokens)
    }

    /// Build from individual built-in or username tokens (e.g. the
    /// elements of a YAML sequence). Each string is one token, taken
    /// verbatim; `team:` references cannot be built this way.
    pub fn from_tokens<Tokens, Token>(tokens: Tokens) -> Self
    where
        Tokens: IntoIterator<Item = Token>,
        Token: AsRef<str>,
    {
        Self(
            tokens
                .into_iter()
                .map(|token| AccessToken::from(token.as_ref()))
                .collect(),
        )
    }

    /// Whether `identity` satisfies any token in the list.
    #[must_use]
    pub fn allows(&self, identity: &Identity) -> bool {
        self.0
            .iter()
            .any(|token| identity.satisfies(token))
    }

    /// Whether `identity` satisfies any token in the list, given whether the
    /// caller is a package maintainer/owner (or admin).
    #[must_use]
    pub fn allows_with_context(&self, identity: &Identity, is_owner: bool) -> bool {
        self.0
            .iter()
            .any(|token| identity.satisfies_with_context(token, is_owner))
    }

    /// Whether the list holds the `$owners` token.
    #[must_use]
    pub fn references_owners(&self) -> bool {
        self.0
            .iter()
            .any(|token| matches!(token, AccessToken::Owners))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Whether the list holds a `team:<team>` token.
    #[must_use]
    pub fn references_team(&self, team: &str) -> bool {
        self.0
            .iter()
            .any(|token| matches!(token, AccessToken::Team { name, .. } if name == team))
    }

    /// Whether the access list admits every caller regardless of identity.
    #[must_use]
    pub fn admits_every_identity(&self) -> bool {
        let mut has_anon = false;
        let mut has_auth = false;
        for token in &self.0 {
            match token {
                AccessToken::All => return true,
                AccessToken::Anonymous => has_anon = true,
                AccessToken::Authenticated => has_auth = true,
                _ => {}
            }
        }
        has_anon && has_auth
    }
}

/// The resolved caller identity an [`AccessList`] is evaluated against: the
/// authenticated username (or its absence), and the team memberships the
/// credential itself carries. Rosters live in the [`AccessToken::Team`]
/// tokens' directory, so a credential that carries no memberships still
/// reaches its roster teams.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// No valid credentials were presented.
    Anonymous,
    /// Authenticated as `username`.
    User { username: String, memberships: Arc<[Membership]> },
}

/// A team membership that a credential carries, such as one an OIDC session
/// earned from its groups claim: membership of `team` in the roster
/// `directory`, whatever the roster itself lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    pub directory: TeamDirectory,
    pub team: String,
}

impl Identity {
    #[must_use]
    pub fn user(username: impl Into<String>) -> Self {
        Self::member(username, Vec::new())
    }

    /// A user whose credential carries `memberships`.
    #[must_use]
    pub fn member(username: impl Into<String>, memberships: Vec<Membership>) -> Self {
        Self::User { username: username.into(), memberships: memberships.into() }
    }

    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        matches!(self, Identity::User { .. })
    }

    #[must_use]
    pub fn username(&self) -> Option<&str> {
        match self {
            Self::Anonymous => None,
            Self::User { username, .. } => Some(username),
        }
    }

    fn satisfies(&self, token: &AccessToken) -> bool {
        self.satisfies_with_context(token, false)
    }

    fn satisfies_with_context(&self, token: &AccessToken, is_owner: bool) -> bool {
        match (token, self) {
            (AccessToken::All, _) => true,
            (AccessToken::Authenticated, Identity::User { .. }) => true,
            (AccessToken::Anonymous, Identity::Anonymous) => true,
            (AccessToken::Owners, Identity::User { .. }) => is_owner,
            (AccessToken::User(name), Identity::User { username, .. }) => name == username,
            (AccessToken::Team { name, directory }, Identity::User { username, memberships }) => {
                directory.has_member(name, username)
                    || memberships
                        .iter()
                        .any(|membership| {
                            membership.directory == *directory && membership.team == *name
                        })
            }
            _ => false,
        }
    }
}

/// The token as it is written in an access list.
impl std::fmt::Display for AccessToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccessToken::All => formatter.write_str("$all"),
            AccessToken::Authenticated => formatter.write_str("$authenticated"),
            AccessToken::Anonymous => formatter.write_str("$anonymous"),
            AccessToken::Owners => formatter.write_str("$owners"),
            AccessToken::User(name) => formatter.write_str(name),
            AccessToken::Team { name, .. } => write!(formatter, "team:{name}"),
        }
    }
}

impl AccessList {
    /// The tokens as they are written in an access list.
    #[must_use]
    pub fn token_strings(&self) -> Vec<String> {
        self.0
            .iter()
            .map(ToString::to_string)
            .collect()
    }
}
