use super::{Diagnostic, Display, Error};

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum AccessError {
    #[display(r#"A subcommand is required (e.g., "list packages", "get status", "set status=public", "grant", "revoke")"#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_SUBCOMMAND_REQUIRED))]
    SubcommandRequired,

    #[display(r#"Unknown subcommand: {cmd}. Run "pnpm help access" for available subcommands."#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_UNKNOWN_SUBCOMMAND))]
    UnknownSubcommand {
        #[error(not(source))]
        cmd: String,
    },

    #[display("Package name is required (e.g., pnpm access get status @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_GET_STATUS_PACKAGE_REQUIRED))]
    GetStatusPackageRequired,

    #[display("Package name is required (e.g., pnpm access list collaborators @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_LIST_COLLABORATORS_PACKAGE_REQUIRED))]
    ListCollaboratorsPackageRequired,

    #[display("Package visibility is required (e.g., pnpm access set status=public @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_STATUS_REQUIRED))]
    SetStatusRequired,

    #[display(r#"Invalid access value "{value}". Must be "public" or "private"."#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_STATUS_INVALID))]
    SetStatusInvalid {
        #[error(not(source))]
        value: String,
    },

    #[display("Package name is required (e.g., pnpm access set status=public @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_STATUS_PACKAGE_REQUIRED))]
    SetStatusPackageRequired,

    #[display(
        "Access settings can only be changed for scoped packages (@scope/name). Unscoped packages are always public."
    )]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_STATUS_UNSCOPED))]
    SetStatusUnscoped,

    #[display("MFA level is required (e.g., pnpm access set mfa=automation @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_MFA_REQUIRED))]
    SetMfaRequired,

    #[display(r#"Invalid MFA value "{value}". Must be "none", "publish", or "automation"."#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_MFA_INVALID))]
    SetMfaInvalid {
        #[error(not(source))]
        value: String,
    },

    #[display("Package name is required (e.g., pnpm access set mfa=automation @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_SET_MFA_PACKAGE_REQUIRED))]
    SetMfaPackageRequired,

    #[display(
        "Permissions and scope:team are required (e.g., pnpm access grant read-only @scope:developers @scope/pkg)"
    )]
    #[diagnostic(code(ERR_PNPM_ACCESS_GRANT_ARGS_REQUIRED))]
    GrantArgsRequired,

    #[display(r#"Invalid permissions "{value}". Must be "read-only" or "read-write"."#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_GRANT_INVALID_PERMISSIONS))]
    GrantInvalidPermissions {
        #[error(not(source))]
        value: String,
    },

    #[display(r#"Invalid team "{team}". Format must be "scope:team". "#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_GRANT_INVALID_TEAM))]
    GrantInvalidTeam {
        #[error(not(source))]
        team: String,
    },

    #[display(
        "Package name is required (e.g., pnpm access grant read-only @scope:developers @scope/pkg)"
    )]
    #[diagnostic(code(ERR_PNPM_ACCESS_GRANT_PACKAGE_REQUIRED))]
    GrantPackageRequired,

    #[display(
        "scope:team and package name are required (e.g., pnpm access revoke @scope:developers @scope/pkg)"
    )]
    #[diagnostic(code(ERR_PNPM_ACCESS_REVOKE_ARGS_REQUIRED))]
    RevokeArgsRequired,

    #[display(r#"Invalid team "{team}". Format must be "scope:team". "#)]
    #[diagnostic(code(ERR_PNPM_ACCESS_REVOKE_INVALID_TEAM))]
    RevokeInvalidTeam {
        #[error(not(source))]
        team: String,
    },

    #[display("Package name is required (e.g., pnpm access revoke @scope:developers @scope/pkg)")]
    #[diagnostic(code(ERR_PNPM_ACCESS_REVOKE_PACKAGE_REQUIRED))]
    RevokePackageRequired,

    #[display(r#"Package "{package_name}" not found in registry"#)]
    #[diagnostic(code(ERR_PNPM_PACKAGE_NOT_FOUND))]
    PackageNotFound {
        #[error(not(source))]
        package_name: String,
    },

    #[display("You must be logged in to {action} packages. {body}")]
    #[diagnostic(code(ERR_PNPM_UNAUTHORIZED))]
    Unauthorized {
        #[error(not(source))]
        action: String,
        #[error(not(source))]
        body: String,
    },

    #[display("You do not have permission to {action} this package. {body}")]
    #[diagnostic(code(ERR_PNPM_FORBIDDEN))]
    Forbidden {
        #[error(not(source))]
        action: String,
        #[error(not(source))]
        body: String,
    },

    #[display("Invalid request: {body}")]
    #[diagnostic(code(ERR_PNPM_ACCESS_VALIDATION_ERROR))]
    ValidationError {
        #[error(not(source))]
        body: String,
    },

    #[display("Failed to {action} package: {status} {status_text}. {body}")]
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

    #[display("Failed to {action} packages: {status} {status_text}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryFetchFailed {
        #[error(not(source))]
        action: String,
        status: u16,
        #[error(not(source))]
        status_text: String,
    },
}
