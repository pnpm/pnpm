use super::registry::{TeamInfo, UserInfo, registry_operation_error};
use miette::IntoDiagnostic;

pub(super) fn render_teams(
    scope: &str,
    teams: &[TeamInfo],
    parseable: bool,
    json: bool,
) -> miette::Result<String> {
    if json {
        let names: Vec<&str> = teams
            .iter()
            .map(|team| team.name.as_str())
            .collect();
        return serde_json::to_string_pretty(&names)
            .into_diagnostic()
            .map_err(|source| registry_operation_error("serializing teams as JSON", source));
    }

    if parseable {
        let lines: Vec<&str> = teams
            .iter()
            .map(|team| team.name.as_str())
            .collect();
        return Ok(lines.join("\n"));
    }

    if teams.is_empty() {
        return Ok(format!("@{scope} has no teams"));
    }

    let mut lines = vec![format!("@{scope} has the following teams:")];
    for team in teams {
        lines.push(format!("  @{scope}:{}", team.name));
    }
    Ok(lines.join("\n"))
}

pub(super) fn render_members(
    scope: &str,
    team: &str,
    members: &[UserInfo],
    parseable: bool,
    json: bool,
) -> miette::Result<String> {
    if json {
        let names: Vec<&str> = members
            .iter()
            .map(|member| member.name.as_str())
            .collect();
        return serde_json::to_string_pretty(&names)
            .into_diagnostic()
            .map_err(|source| registry_operation_error("serializing members as JSON", source));
    }

    if parseable {
        let lines: Vec<&str> = members
            .iter()
            .map(|member| member.name.as_str())
            .collect();
        return Ok(lines.join("\n"));
    }

    if members.is_empty() {
        return Ok(format!("@{scope}:{team} has no members"));
    }

    let mut lines = vec![format!("@{scope}:{team} has the following members:")];
    for member in members {
        lines.push(format!("  {}", member.name));
    }
    Ok(lines.join("\n"))
}
