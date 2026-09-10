use super::{AccessList, AccessToken, BTreeSet, Deserialize, IndexMap, RegistryError};

/// One `packages:` map value: `access` / `publish` / `unpublish` are
/// permission lists (the built-in `$all` / `$authenticated` / `$anonymous`
/// pseudo-groups, bare usernames, and `team:<name>` references to the owning
/// registry's declared teams), compiled into the owning registry's
/// [`pnpr_policy::PackageRules`]. An omitted field falls back to the registry-level
/// default. The map key set doubles as the registry's declared namespace, so
/// one declaration routes, filters, and authorizes.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageAccess {
    pub access: Option<AccessSpec>,
    pub publish: Option<AccessSpec>,
    pub unpublish: Option<AccessSpec>,
}

/// A YAML permission value: a sequence of tokens, or a scalar naming exactly
/// one token. Every entry is taken verbatim — there is no whitespace
/// splitting inside a scalar or a sequence element, so a multi-token list
/// must be a YAML sequence (`access: [$authenticated, admin]`). Verdaccio's
/// space-separated form (`access: $authenticated admin`) is rejected with a
/// pointer at the sequence syntax rather than silently misread as one token.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AccessSpec {
    One(String),
    Many(Vec<String>),
}

impl AccessSpec {
    /// The declared entries: the scalar form is one entry, the sequence
    /// form one per element.
    pub(super) fn entries(&self) -> &[String] {
        match self {
            AccessSpec::One(entry) => std::slice::from_ref(entry),
            AccessSpec::Many(entries) => entries,
        }
    }

    /// Compile into an [`AccessList`], rejecting any entry that is not a
    /// single well-formed token ([`validate_access_token`]) and resolving
    /// `team:` references against the owning registry's declared `teams` —
    /// an undeclared team is an error, so a typo cannot silently become a
    /// grant to nobody. The returned error is the reason only; the caller
    /// prefixes the registry/field context it alone knows.
    pub(super) fn to_access_list(&self, teams: &Teams) -> Result<AccessList, String> {
        let mut tokens = Vec::with_capacity(self.entries().len());
        for entry in self.entries() {
            validate_access_token(entry)?;
            tokens.push(match entry.strip_prefix("team:") {
                Some(team) => {
                    let members = teams.get(team).ok_or_else(|| {
                        format!(
                            "access token {entry:?} references a team this registry does not \
                             declare{}",
                            declared_teams(teams),
                        )
                    })?;
                    AccessToken::Team { name: team.to_string(), members: members.clone() }
                }
                None => AccessToken::from(entry.as_str()),
            });
        }
        Ok(AccessList::new(tokens))
    }

    /// The `teams:` membership reading: each entry is one username
    /// ([`validate_member_name`]), in declared order. Unlike
    /// [`Self::to_access_list`] the entries are not access tokens — a
    /// built-in group or `team:` reference among the members is an error.
    pub(super) fn member_names(&self) -> Result<&[String], String> {
        for member in self.entries() {
            validate_member_name(member)?;
        }
        Ok(self.entries())
    }
}

/// A registry's declared teams — its `teams:` map compiled to name →
/// member-set. Access lists capture the member sets they reference at
/// compile time (see [`AccessToken::Team`]); a hosted registry additionally
/// retains its map on [`crate::HostedConfig::teams`] so the npm team API can list
/// teams and their members.
pub type Teams = IndexMap<String, BTreeSet<String>>;

/// The declared team names for an undeclared-reference error, so a typo'd
/// `team:` token points at what exists.
pub(super) fn declared_teams(teams: &Teams) -> String {
    if teams.is_empty() {
        return " (it declares no `teams:`)".to_string();
    }
    let names = teams.keys().map(|name| format!("{name:?}")).collect::<Vec<_>>().join(", ");
    format!("; its declared teams are {names}")
}

/// Compile one registry's `teams:` map, validating team names (they must be
/// writable after `team:` in a token) and member lists.
pub(super) fn build_teams(
    registry: &str,
    file: &IndexMap<String, AccessSpec>,
) -> Result<Teams, RegistryError> {
    let mut teams = Teams::default();
    for (team, members) in file {
        validate_team_name(team).map_err(|reason| RegistryError::InvalidConfig {
            reason: format!("registry {registry:?} has an invalid team name: {reason}"),
        })?;
        let members = members.member_names().map_err(|reason| RegistryError::InvalidConfig {
            reason: format!(
                "registry {registry:?} team {team:?} has an invalid member list: {reason}",
            ),
        })?;
        teams.insert(team.clone(), members.iter().cloned().collect());
    }
    Ok(teams)
}

/// A team name is only useful spliced into a `team:<name>` token, so it must
/// survive that grammar: one token, no `:` (which would read as another
/// prefix), no `$` sigil (reserved for the built-in groups).
pub(super) fn validate_team_name(team: &str) -> Result<(), String> {
    validate_single_token(team)?;
    if team.contains(':') || team.starts_with('$') {
        return Err(format!(
            "team name {team:?} cannot contain `:` or start with `$`; it is referenced from \
             access lists as `team:{team}`",
        ));
    }
    Ok(())
}

/// A team member is one plain username. The built-in groups — in any
/// spelling — and typed tokens are rejected: `[$authenticated]` in a member
/// list would otherwise silently become a user literally named
/// `$authenticated`, narrowing the team to a name nobody holds, the same
/// trap [`validate_access_token`] closes for access lists.
pub(super) fn validate_member_name(member: &str) -> Result<(), String> {
    validate_single_token(member)?;
    let bare = member.strip_prefix('@').unwrap_or(member);
    if member.starts_with('$') || matches!(bare, "all" | "authenticated" | "anonymous") {
        return Err(format!(
            "team member {member:?} is not a username; the built-in groups belong in the \
             access lists themselves (e.g. `access: [$authenticated]`)",
        ));
    }
    if member.contains(':') {
        return Err(format!(
            "team member {member:?} is not a username; a team cannot include another team — \
             list its users, or share one roster with a YAML anchor",
        ));
    }
    Ok(())
}

/// Reject an access-list entry that is not exactly one recognized token: an
/// unknown `$...` built-in, an unknown `<type>:` prefix (only `team:` exists;
/// htpasswd forbids `:` in usernames, so the character is free to reserve),
/// or one of verdaccio's alias spellings of the built-ins (`@all`, bare
/// `all`, ...), which must not silently become a username that admits nobody.
/// This loud rejection at the YAML boundary is what lets `AccessToken`
/// parsing stay infallible.
pub(super) fn validate_access_token(token: &str) -> Result<(), String> {
    validate_single_token(token)?;
    if let Some(builtin) = token.strip_prefix('$') {
        if !matches!(builtin, "all" | "authenticated" | "anonymous") {
            return Err(format!(
                "unknown built-in access token {token:?}; the built-in groups are `$all`, \
                 `$authenticated`, and `$anonymous`",
            ));
        }
        return Ok(());
    }
    if let Some((kind, name)) = token.split_once(':') {
        return match kind {
            "team" if name.contains(':') => {
                Err(format!("access token {token:?} is malformed; a team name cannot contain `:`"))
            }
            "team" if !name.is_empty() => Ok(()),
            "team" => Err(format!("access token {token:?} names no team; write `team:<name>`")),
            "group" | "groups" => Err(format!(
                "unknown access token type {kind:?} in {token:?}; teams are declared per \
                 registry — did you mean \"team:{name}\"?",
            )),
            _ => Err(format!(
                "unknown access token type {kind:?} in {token:?}; the only typed token is \
                 `team:<name>` (a bare token is a username)",
            )),
        };
    }
    let bare = token.strip_prefix('@').unwrap_or(token);
    if matches!(bare, "all" | "authenticated" | "anonymous") {
        return Err(format!(r#"unknown access token {token:?}; did you mean "${bare}"?"#));
    }
    Ok(())
}

/// Reject a value that is not exactly one token: the empty string, or a
/// string containing whitespace (a space-separated list must be a YAML
/// sequence instead).
pub(super) fn validate_single_token(token: &str) -> Result<(), String> {
    if token.is_empty() {
        return Err(
            "an empty string is not a token; use `[]` to admit no one, or omit the field for \
             the default"
                .to_string(),
        );
    }
    if token.contains(char::is_whitespace) {
        return Err(format!(
            "{token:?} contains whitespace; write one token per YAML sequence item \
             (e.g. `[alice, bob]`)",
        ));
    }
    Ok(())
}
