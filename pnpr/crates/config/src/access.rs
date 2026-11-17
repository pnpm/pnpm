use super::{AccessList, AccessToken, BTreeSet, Deserialize, RegistryError, Serialize};

/// A YAML permission value: a sequence of tokens, or a scalar naming exactly
/// one token. Every entry is taken verbatim — there is no whitespace
/// splitting inside a scalar or a sequence element, so a multi-token list
/// must be a YAML sequence (`access: [$authenticated, admin]`). Verdaccio's
/// space-separated form (`access: $authenticated admin`) is rejected with a
/// pointer at the sequence syntax rather than silently misread as one token.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// single well-formed token ([`validate_access_token`]). A `team:<id>`
    /// entry is not checked against the teams that exist: a team that does
    /// not exist has no members. The returned error is the reason only; the
    /// caller prefixes the registry/field context it alone knows.
    pub(super) fn to_access_list(&self) -> Result<AccessList, String> {
        let mut tokens = Vec::with_capacity(self.entries().len());
        for entry in self.entries() {
            validate_access_token(entry)?;
            tokens.push(match entry.strip_prefix("team:") {
                Some(id) => AccessToken::Team(id.to_string()),
                None => AccessToken::from(entry.as_str()),
            });
        }
        Ok(AccessList::new(tokens))
    }
}

/// Compile `auth.admins`, rejecting any entry that is not one account ID.
/// The account IDs of an `auth` admin list; `field` names the list in errors.
pub(super) fn build_admins(
    field: &str,
    names: &[String],
) -> Result<BTreeSet<String>, RegistryError> {
    names
        .iter()
        .map(|name| {
            validate_member_name(name)
                .map(|()| name.clone())
                .map_err(|reason| RegistryError::InvalidConfig {
                    reason: format!("`{field}` has an invalid entry: {reason}"),
                })
        })
        .collect()
}

/// Validate the single-token grammar shared by team names and IDs.
pub fn validate_team_name(team: &str) -> Result<(), String> {
    validate_single_token(team)?;
    if team.contains(':') || team.starts_with('$') {
        return Err(format!(
            "team name {team:?} cannot contain `:` or start with `$`; it is referenced from \
             access lists as `team:{team}`",
        ));
    }
    Ok(())
}

/// Validate one literal account reference, rejecting group and team tokens.
pub fn validate_member_name(member: &str) -> Result<(), String> {
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
            "team member {member:?} is not a username; a team cannot include another team, so \
             list its users",
        ));
    }
    Ok(())
}

/// Reject unknown built-ins and typed prefixes, reserved aliases, and malformed IDs.
pub fn validate_access_token(token: &str) -> Result<(), String> {
    validate_single_token(token)?;
    if let Some(builtin) = token.strip_prefix('$') {
        if !matches!(builtin, "all" | "authenticated" | "anonymous" | "owners") {
            return Err(format!(
                "unknown built-in access token {token:?}; the built-in groups are `$all`, \
                 `$authenticated`, `$anonymous`, and `$owners`",
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
            "team" => Err(format!("access token {token:?} names no team; write `team:<id>`")),
            "group" | "groups" => Err(format!(
                r#"unknown access token type {kind:?} in {token:?}; did you mean "team:{name}"?"#,
            )),
            _ => Err(format!(
                "unknown access token type {kind:?} in {token:?}; the only typed token is \
                 `team:<id>` (a bare token is an account ID)",
            )),
        };
    }
    let bare = token.strip_prefix('@').unwrap_or(token);
    if matches!(bare, "all" | "authenticated" | "anonymous" | "owners") {
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
