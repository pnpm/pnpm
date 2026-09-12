use super::{Host, Path, RunCommand, Version, VersionArgs, VersionChange, VersionError};

/// Build the canonical cross-stack error for an invalid version from Git.
fn invalid_version_from_git(
    cwd: &Path,
    tag_version_prefix: &str,
    reason: impl Into<String>,
) -> VersionError {
    VersionError::InvalidVersionFromGit {
        dir: cwd.display().to_string(),
        tag_version_prefix: tag_version_prefix.to_string(),
        reason: reason.into(),
    }
}

pub(super) fn version_from_git(
    cwd: &Path,
    tag_version_prefix: &str,
) -> Result<Version, VersionError> {
    let pattern = format!("{tag_version_prefix}*.*.*");
    let args = ["describe", "--tags", "--abbrev=0", "--always", "--match", pattern.as_str()];
    let output = <Host as RunCommand>::run("git", &args, Some(cwd)).map_err(|err| {
        VersionError::GitCommandFailed { args: args.join(" "), stderr: err.to_string() }
    })?;

    if !output.success {
        return Err(VersionError::GitCommandFailed {
            args: args.join(" "),
            stderr: output.stderr.trim().to_string(),
        });
    }

    let tag = output.stdout.trim();
    let tag_args = ["tag", "--list", "--", tag];
    let matching_tag = <Host as RunCommand>::run("git", &tag_args, Some(cwd)).map_err(|err| {
        VersionError::GitCommandFailed { args: tag_args.join(" "), stderr: err.to_string() }
    })?;

    if !matching_tag.success {
        return Err(VersionError::GitCommandFailed {
            args: tag_args.join(" "),
            stderr: matching_tag.stderr.trim().to_string(),
        });
    }

    if matching_tag.stdout.trim() != tag {
        return Err(invalid_version_from_git(cwd, tag_version_prefix, "no matching Git tag found"));
    }

    let Some(raw_version) = tag.strip_prefix(tag_version_prefix) else {
        return Err(invalid_version_from_git(
            cwd,
            tag_version_prefix,
            format!("tag is not a valid version: {tag:?}"),
        ));
    };

    Version::parse(raw_version).map_err(|_| {
        invalid_version_from_git(
            cwd,
            tag_version_prefix,
            format!("tag is not a valid version: {tag:?}"),
        )
    })
}

/// Run a git command in `cwd`, failing with the command line and git's stderr
/// when it exits non-zero.
fn run_git(cwd: &Path, args: &[&str]) -> miette::Result<()> {
    let output = <Host as RunCommand>::run("git", args, Some(cwd)).map_err(|err| {
        VersionError::GitCommandFailed { args: args.join(" "), stderr: err.to_string() }
    })?;
    if !output.success {
        return Err(VersionError::GitCommandFailed {
            args: args.join(" "),
            stderr: output.stderr.trim().to_string(),
        }
        .into());
    }
    Ok(())
}

impl VersionArgs {
    /// Stage the bumped manifest and record the bump as a commit plus an
    /// annotated (or signed) tag, mirroring the TypeScript `commitAndTag`.
    pub(super) fn commit_and_tag(&self, change: &VersionChange, cwd: &Path) -> miette::Result<()> {
        let message = self.message.as_deref().unwrap_or("%s").replace("%s", &change.new_version);
        let tag_name = format!("{}{}", self.tag_version_prefix, change.new_version);

        let Ok(relative) = change.manifest_path.strip_prefix(cwd) else {
            return Err(VersionError::InvalidManifestPath {
                path: change.manifest_path.display().to_string(),
            }
            .into());
        };
        let manifest_rel: String = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");

        run_git(cwd, &["add", &manifest_rel])?;

        let mut commit_args = vec!["commit", "-m", &message];
        if self.no_commit_hooks {
            commit_args.push("--no-verify");
        }
        // The manifest write can leave nothing staged on an
        // --allow-same-version run. Pass --allow-empty in that case to let
        // the tag point at the current HEAD as a deliberate marker.
        if self.allow_same_version {
            commit_args.push("--allow-empty");
        }
        run_git(cwd, &commit_args)?;

        let mut tag_args = vec!["tag", if self.sign_git_tag { "-s" } else { "-a" }];
        tag_args.extend([tag_name.as_str(), "-m", &message]);
        run_git(cwd, &tag_args)
    }
}
