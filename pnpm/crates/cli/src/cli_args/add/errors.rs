use super::{Diagnostic, Display, Error};

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum AddError {
    #[display(
        "Running this command will add the dependency to the workspace root, which might not be what you want - if you really meant it, make it explicit by running this command again with the -w flag (or --workspace-root). If you don't want to see this warning anymore, you may set the ignore-workspace-root-check setting to true."
    )]
    #[diagnostic(code(ERR_PNPM_ADDING_TO_ROOT))]
    AddingToRoot,

    #[display(
        "Cannot declare {request} as the package manager of a filtered selection of projects"
    )]
    #[diagnostic(
        code(ERR_PNPM_PACKAGE_MANAGER_IN_SELECTION),
        help(
            "Which package manager a project uses is declared in that project. Run the command in the project itself, without a filter."
        )
    )]
    PackageManagerInSelection {
        #[error(not(source))]
        request: String,
    },

    /// A `--workspace` selector named a package that no workspace project
    /// publishes.
    #[display(r#""{name}" not found in the workspace"#)]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_PACKAGE_NOT_FOUND))]
    WorkspacePackageNotFound {
        #[error(not(source))]
        name: String,
    },

    /// A `--workspace` selector carried no package name to look up in the
    /// workspace, such as a bare path or URL.
    #[display(r#"Cannot update/install from workspace through "{selector}""#)]
    #[diagnostic(code(ERR_PNPM_NO_PKG_NAME_IN_SPEC))]
    NoPkgNameInSpec {
        #[error(not(source))]
        selector: String,
    },
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum AllowBuildError {
    #[display(
        "The following dependencies are ignored by the root project, but are allowed to be built by the current command: {dependencies}"
    )]
    #[diagnostic(
        code(ERR_PNPM_OVERRIDING_IGNORED_BUILT_DEPENDENCIES),
        help(
            "If you are sure you want to allow those dependencies to run installation scripts, remove them from the allowBuilds list (or change their value to true)."
        )
    )]
    OverridingIgnoredBuiltDependencies { dependencies: String },

    #[display(
        "The --allow-build flag is missing a package name. Please specify the package name(s) that are allowed to run installation scripts."
    )]
    #[diagnostic(code(ERR_PNPM_ALLOW_BUILD_MISSING_PACKAGE))]
    MissingPackage,
}
