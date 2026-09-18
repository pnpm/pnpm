use super::{Diagnostic, Display, Error, PathBuf, io};

/// Error when reading `pnpm-workspace.yaml`.
///
/// `ENOENT` is treated as "no manifest" and every other failure
/// propagates. `serde_saphyr::Error` is boxed so the returned
/// `Result` stays small.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LoadWorkspaceYamlError {
    #[display("Failed to read pnpm-workspace.yaml at {}: {source}", path.display())]
    ReadFile {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("Failed to parse pnpm-workspace.yaml at {}: {source}", path.display())]
    ParseYaml {
        path: PathBuf,
        #[error(source)]
        source: Box<serde_saphyr::Error>,
    },
    /// The registry URL is redacted before it reaches this variant.
    #[display("The \"registries\" key {registry} embeds credentials")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help("Put them in an .npmrc file instead, so they are not committed.")
    )]
    CredentialsInRegistryKey { registry: String },
    /// The registry URL is redacted before it reaches this variant.
    #[display(
        r#"The "registries[{registry:?}].{field}" setting is not allowed in pnpm-workspace.yaml"#
    )]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help("Set it in an .npmrc file instead, so it is not committed.")
    )]
    SecretInRegistryDeclaration { registry: String, field: String },
    /// The registry URL is redacted before it reaches this variant.
    #[display(r#"The "registries[{registry:?}].{field}" setting is not a known registry setting"#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help(r#"A registry declares "serverType", "scopes", and "prefix"."#)
    )]
    UnknownRegistryDeclarationField { registry: String, field: String },
    #[display(r#"The "registries" setting mixes registry declarations with "<scope>: <url>" entries ({scopes})"#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help(
            r#"Key every entry by registry URL and list the scopes routed to it under "scopes"."#
        )
    )]
    MixedRegistriesShapes { scopes: String },
    /// The registry URL is redacted before it reaches this variant.
    #[display(r#"The "registries[{registry:?}]" entry is a string"#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help(
            r#"A registry URL keys a declaration, e.g. {{ serverType: "artifactory" }}. A string value routes a scope, and a URL is not a scope."#
        )
    )]
    StringValuedRegistryDeclaration { registry: String },
    /// The registry URL is redacted before it reaches this variant.
    #[display(r#"The "registries[{registry:?}].scopes" setting should list "@"-prefixed scopes, but got {scope:?}"#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help(r#"A bare "@" is the scope-less default registry."#)
    )]
    RegistryScopeWithoutAtSign { registry: String, scope: String },
    /// The registry URLs are redacted before they reach this variant.
    #[display("The scope {scope:?} is routed to two registries: {registries}")]
    #[diagnostic(code(ERR_PNPM_INVALID_SETTING))]
    ScopeRoutedTwice { scope: String, registries: String },
    #[display("The prefix {prefix:?} is declared by two registries")]
    #[diagnostic(code(ERR_PNPM_INVALID_SETTING))]
    PrefixDeclaredTwice { prefix: String },
    /// The registry URL is redacted before it reaches this variant.
    #[display(r#"The "registries[{registry:?}].{field}" setting is for an npm registry, but the entry serves {ecosystem}"#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help(r#"Scopes, bare-specifier prefixes and server descriptions are npm's. Drop the field, or drop "ecosystem" to declare an npm registry."#)
    )]
    EcosystemRegistryDeclaresNpmField { registry: String, ecosystem: String, field: String },
    /// The registry URL is redacted before it reaches this variant.
    #[display("The {ecosystem} index {registry:?} is declared twice")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help(
            "Two keys that differ only by a trailing slash address the same index. Declare it once."
        )
    )]
    EcosystemIndexDeclaredTwice { ecosystem: String, registry: String },
    /// The registry URLs are redacted before they reach this variant.
    #[display("Two Cargo registries are declared: {registries}")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help("pnpm resolves Cargo dependencies from one sparse index. Declare the one to use.")
    )]
    CargoIndexDeclaredTwice { registries: String },
    #[display("Invalid Python package routes for {registry:?}: {reason}")]
    #[diagnostic(code(ERR_PNPM_INVALID_SETTING))]
    InvalidPythonRegistryPackages { registry: String, reason: String },
    #[display("The Python package pattern {pattern:?} is routed to two registries: {registries}")]
    #[diagnostic(code(ERR_PNPM_INVALID_SETTING))]
    PythonPackageRoutedTwice { pattern: String, registries: String },
    #[display("The \"pipelines['{pipeline}']\" setting contains an entry with no task name")]
    #[diagnostic(code(ERR_PNPM_INVALID_SETTING))]
    EmptyPipelineTaskName { pipeline: String },
    #[display(
        "The \"tasks['{task}'].concurrency\" setting should be a positive integer, but got {concurrency}"
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_SETTING))]
    InvalidTaskConcurrency { task: String, concurrency: String },
    #[display(
        "The \"tasks['{task}'].concurrencyGroup\" setting is not a valid group name: {group:?}"
    )]
    #[diagnostic(
        code(ERR_PNPM_INVALID_SETTING),
        help(
            "A group name is one or more letters, digits, '.', '_' or '-', and names a slot directory of its own, so it cannot be '.', '..', a Windows device name, end with '.', or contain a path separator."
        )
    )]
    InvalidTaskConcurrencyGroup { task: String, group: String },
    #[display(
        "The \"tasks['{task}'].dependsOn\" setting contains an entry with no task name: {entry:?}"
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_SETTING))]
    EmptyTaskDependsOnEntry { task: String, entry: String },
    #[display("Invalid `_auth` setting: {source}")]
    InvalidJsonAuth {
        #[error(source)]
        source: serde_json::Error,
    },
    /// A `tokenHelper` was configured in a workspace or project `.npmrc`.
    /// It names an executable, so it is only honored from a trusted,
    /// non-repo source (`~/.npmrc` or the global `auth.ini`); a
    /// checked-in `.npmrc` must not be able to run an arbitrary command.
    #[display("tokenHelper must not be configured in project-level .npmrc")]
    #[diagnostic(
        code(ERR_PNPM_TOKEN_HELPER_IN_PROJECT_CONFIG),
        help(
            "The key {key:?} was found in project config. Move it to ~/.npmrc or the global pnpm auth.ini."
        )
    )]
    TokenHelperInProjectConfig { key: String },
    /// An `_auth` credential did not decode as base64. Its whole point is
    /// to carry `<username>:<password>` base64-encoded, so a value that
    /// cannot be decoded would otherwise reach the registry as a header
    /// no server can read — a silent 401 instead of a fixable error.
    #[display("Failed to decode {key} as base64")]
    #[diagnostic(
        code(ERR_PNPM_AUTH_INVALID_BASE64),
        help("{key} must hold the base64 encoding of <username>:<password>.")
    )]
    AuthInvalidBase64 { key: &'static str },
    /// A decoded `_auth` credential held no `:`, so it names no password.
    #[display("No separator found in the decoded form of _auth")]
    #[diagnostic(
        code(ERR_PNPM_AUTH_MISSING_SEPARATOR),
        help(
            "_auth is a base64 encoded form of <username>:<password> where the colon (:) serves as the separator"
        )
    )]
    AuthMissingSeparator,
    /// A honored `tokenHelper` value contained a character pnpm reserves
    /// for future quoting / interpolation support.
    #[display("Unexpected character {character:?} in tokenHelper")]
    #[diagnostic(
        code(ERR_PNPM_TOKEN_HELPER_UNSUPPORTED_CHARACTER),
        help(
            "Try wrapping the current command in a script whose name does not contain unsupported characters."
        )
    )]
    TokenHelperUnsupportedCharacter { character: char },
    /// The root manifest a `$dep-name` self-reference in `overrides`
    /// resolves against exists but could not be read or parsed.
    /// Boxed so the returned `Result` stays small.
    #[display("Failed to read the root package.json: {source}")]
    ReadRootManifest {
        #[error(source)]
        source: Box<pnpm_package_manifest::PackageManifestError>,
    },
    /// An `overrides` value used the `$dep-name` self-reference syntax,
    /// but the root manifest declares no such direct dependency.
    #[display(
        r#"Cannot resolve version {spec} in overrides. The direct dependencies don't have dependency "{dependency_name}"."#
    )]
    #[diagnostic(code(ERR_PNPM_CANNOT_RESOLVE_OVERRIDE_VERSION))]
    CannotResolveOverrideVersion { spec: String, dependency_name: String },

    /// The signing trust root for remote side-effects artifacts appeared in a
    /// committed file. Only the global config yaml and the environment may
    /// carry it — see [`RemoteSideEffectsCacheSettings`](crate::workspace_yaml::sections::RemoteSideEffectsCacheSettings).
    #[display("{prefix}.{field} cannot be set by a workspace ({})", path.display())]
    #[diagnostic(
        code(ERR_PNPM_WORKSPACE_REMOTE_SIDE_EFFECTS_TRUST),
        help(
            "Set it in the global config file or in the environment instead of {}.",
            path.display(),
        )
    )]
    WorkspaceRemoteSideEffectsTrust { path: PathBuf, prefix: &'static str, field: &'static str },
}
