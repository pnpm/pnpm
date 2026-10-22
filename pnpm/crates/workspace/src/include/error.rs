use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{io, path::PathBuf};

/// Why the include index of `pnpm-workspace.yaml`, or a file it includes,
/// cannot be loaded.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum IncludeError {
    #[display("Failed to read pnpm-workspace.yaml at {}: {source}", path.display())]
    ReadRoot {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to parse pnpm-workspace.yaml at {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    ParseRoot {
        path: PathBuf,
        #[error(source)]
        source: yaml_serde::Error,
    },

    #[display(
        r#""include" in pnpm-workspace.yaml must map each source to a list of settings or "*""#
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    IndexNotAMapping,

    #[display(
        r#"The include source "{spec}" is neither a relative path starting with "./" or "../" nor a config dependency name"#
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    InvalidSource {
        #[error(not(source))]
        spec: String,
    },

    #[display(r#"The include source "{spec}" is not a valid file path: {reason}"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    InvalidLocalPath { spec: String, reason: &'static str },

    #[display(r#"The include source "{spec}" must be delegated a list of settings or "*""#)]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    InvalidDelegation {
        #[error(not(source))]
        spec: String,
    },

    #[display(r#""{selector}" in the settings delegated to "{spec}" is not a setting name"#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE),
        help(r#"Nested selectors are supported only as "catalogs.<name>"."#)
    )]
    InvalidSelector { spec: String, selector: String },

    #[display(r#""{selector}" is listed more than once in the settings delegated to "{spec}""#)]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    RepeatedSelector { spec: String, selector: String },

    #[display(r#"The config dependency "{name}" must be delegated a list of settings, not "*""#)]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    UnrestrictedPackage {
        #[error(not(source))]
        name: String,
    },

    #[display(r#""{setting}" cannot be delegated to the config dependency "{name}""#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE),
        help(
            "A config dependency cannot control configuration loading, select hooks, govern its own installation, or set repository paths."
        )
    )]
    ForbiddenPackageSetting { name: String, setting: String },

    #[display(r#"The include source "{name}" is not declared in configDependencies"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    UndeclaredConfigDependency {
        #[error(not(source))]
        name: String,
    },

    #[display(r#"Failed to read the included file "{spec}": {source}"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    ReadFragment {
        spec: String,
        #[error(source)]
        source: io::Error,
    },

    #[display(r#"The included path "{spec}" is not a file"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    NotAFile {
        #[error(not(source))]
        spec: String,
    },

    #[display(r#"Failed to parse the included file "{spec}": {source}"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    ParseFragment {
        spec: String,
        #[error(source)]
        source: yaml_serde::Error,
    },

    #[display(r#"The included file "{spec}" must contain a mapping of settings"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    FragmentNotAMapping {
        #[error(not(source))]
        spec: String,
    },

    #[display(r#"The included file "{spec}" defines "include". Only pnpm-workspace.yaml can include files"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    NestedInclude {
        #[error(not(source))]
        spec: String,
    },

    #[display(r#"The include source "{spec}" is pnpm-workspace.yaml itself"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    IncludesRoot {
        #[error(not(source))]
        spec: String,
    },

    #[display(r#"The include sources "{other}" and "{spec}" name the same file"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_INCLUDE))]
    DuplicateSource { spec: String, other: String },

    #[display(r#"Failed to read the config dependency "{name}": {source}"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_CONFIG_DEPENDENCY_ENTRY))]
    ReadPackage {
        name: String,
        #[error(source)]
        source: io::Error,
    },

    #[display(r#"Failed to parse the package.json of the config dependency "{name}": {source}"#)]
    #[diagnostic(code(ERR_PNPM_INVALID_CONFIG_DEPENDENCY_ENTRY))]
    ParsePackageManifest {
        name: String,
        #[error(source)]
        source: serde_json::Error,
    },

    #[display(r#"The config dependency "{name}" is included in pnpm-workspace.yaml but its package.json does not declare "pnpm.config""#)]
    #[diagnostic(code(ERR_PNPM_INVALID_CONFIG_DEPENDENCY_ENTRY))]
    MissingPackageEntry {
        #[error(not(source))]
        name: String,
    },

    #[display(
        r#"The config entry "{entry}" of the config dependency "{name}" is invalid: {reason}"#
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_CONFIG_DEPENDENCY_ENTRY))]
    InvalidPackageEntry { name: String, entry: String, reason: &'static str },

    #[display(
        r#""{setting}" is {first} but is also {second}. Each setting can have one owner only."#
    )]
    #[diagnostic(code(ERR_PNPM_DUPLICATE_SETTING))]
    DuplicateSetting { setting: String, first: String, second: String },

    #[display(r#""{setting}" is defined in {spec}, which is not delegated that setting"#)]
    #[diagnostic(
        code(ERR_PNPM_SETTING_NOT_DELEGATED),
        help(r#"Add "{setting}" to the settings "include" delegates to {spec} in pnpm-workspace.yaml, or move it to the source that owns it."#)
    )]
    SettingNotDelegated { setting: String, spec: String },
}
