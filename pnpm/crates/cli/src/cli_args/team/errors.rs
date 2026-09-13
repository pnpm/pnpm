use super::{Diagnostic, Display, Error};

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum TeamError {
    #[display(
        "Subcommand is required (create, destroy, add, rm, ls). Use `pnpm team ls <scope>` to list teams."
    )]
    #[diagnostic(code(ERR_PNPM_TEAM_SUBCOMMAND_REQUIRED))]
    SubcommandRequired,

    #[display(
        r#"Team spec must start with @scope, got "{spec}". Use @scope or @scope:team format."#
    )]
    #[diagnostic(code(ERR_PNPM_TEAM_INVALID_SCOPE))]
    InvalidScope {
        #[error(not(source))]
        spec: String,
    },

    #[display("Team scope is required (e.g., pnpm team create @org:newteam)")]
    #[diagnostic(code(ERR_PNPM_TEAM_CREATE_SCOPE_REQUIRED))]
    CreateScopeRequired,

    #[display("Team name is required (e.g., pnpm team create @org:newteam)")]
    #[diagnostic(code(ERR_PNPM_TEAM_CREATE_NAME_REQUIRED))]
    CreateNameRequired,

    #[display("Team scope is required (e.g., pnpm team destroy @org:newteam)")]
    #[diagnostic(code(ERR_PNPM_TEAM_DESTROY_SCOPE_REQUIRED))]
    DestroyScopeRequired,

    #[display("Team name is required (e.g., pnpm team destroy @org:newteam)")]
    #[diagnostic(code(ERR_PNPM_TEAM_DESTROY_NAME_REQUIRED))]
    DestroyNameRequired,

    #[display("Team scope and user are required (e.g., pnpm team add @org:team username)")]
    #[diagnostic(code(ERR_PNPM_TEAM_ADD_ARGS_REQUIRED))]
    AddArgsRequired,

    #[display("Team name is required (e.g., pnpm team add @org:team username)")]
    #[diagnostic(code(ERR_PNPM_TEAM_ADD_NAME_REQUIRED))]
    AddNameRequired,

    #[display("Team scope and user are required (e.g., pnpm team rm @org:team username)")]
    #[diagnostic(code(ERR_PNPM_TEAM_RM_ARGS_REQUIRED))]
    RmArgsRequired,

    #[display("Team name is required (e.g., pnpm team rm @org:team username)")]
    #[diagnostic(code(ERR_PNPM_TEAM_RM_NAME_REQUIRED))]
    RmNameRequired,

    #[display("Organization scope is required (e.g., pnpm team ls @org or pnpm team ls @org:team)")]
    #[diagnostic(code(ERR_PNPM_TEAM_LS_SCOPE_REQUIRED))]
    LsScopeRequired,

    #[display(r#"Organization "@{scope}" not found in registry"#)]
    #[diagnostic(code(ERR_PNPM_ORG_NOT_FOUND))]
    OrgNotFound {
        #[error(not(source))]
        scope: String,
    },

    #[display(r#"Team "@{scope}:{team}" not found in registry"#)]
    #[diagnostic(code(ERR_PNPM_TEAM_NOT_FOUND))]
    TeamNotFound {
        #[error(not(source))]
        scope: String,
        #[error(not(source))]
        team: String,
    },

    #[display("You must be logged in to {action}. {body}")]
    #[diagnostic(code(ERR_PNPM_UNAUTHORIZED))]
    Unauthorized {
        #[error(not(source))]
        action: String,
        #[error(not(source))]
        body: String,
    },

    #[display("You do not have permission to {action}. {body}")]
    #[diagnostic(code(ERR_PNPM_FORBIDDEN))]
    Forbidden {
        #[error(not(source))]
        action: String,
        #[error(not(source))]
        body: String,
    },

    #[display("Authentication required for registry access")]
    #[diagnostic(code(ERR_PNPM_TEAM_MISSING_AUTH))]
    MissingAuthToken,

    #[display("Organization or team not found. {body}")]
    #[diagnostic(code(ERR_PNPM_NOT_FOUND))]
    NotFound {
        #[error(not(source))]
        body: String,
    },

    #[display("Team operation failed due to conflict. {body}")]
    #[diagnostic(code(ERR_PNPM_TEAM_CONFLICT))]
    Conflict {
        #[error(not(source))]
        body: String,
    },

    #[display("Failed to {action}: {status} {status_text}. {body}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryWriteFailed {
        #[error(not(source))]
        action: String,
        status: u16,
        #[error(not(source))]
        status_text: String,
        #[error(not(source))]
        body: String,
    },

    #[display("Failed to {operation}: {reason}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryOperationFailed {
        #[error(not(source))]
        operation: &'static str,
        #[error(not(source))]
        reason: String,
    },
}
