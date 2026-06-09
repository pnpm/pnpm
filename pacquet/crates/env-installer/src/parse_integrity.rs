use crate::ConfigDepError;
use ssri::Integrity;

/// One configurational dependency normalized for installation — the
/// shape the install pass consumes after reading the env lockfile (or
/// migrating an old inline-integrity manifest entry). Mirrors pnpm's
/// [`NormalizedConfigDep`](https://github.com/pnpm/pnpm/blob/31858c544b/installing/env-installer/src/parseIntegrity.ts).
#[derive(Debug, Clone)]
pub struct NormalizedConfigDep {
    pub version: String,
    pub integrity: Integrity,
    pub tarball: String,
    pub optional_subdeps: Vec<NormalizedSubdep>,
}

/// A platform-specific optional subdependency of a config dependency.
/// Mirrors pnpm's
/// [`NormalizedSubdep`](https://github.com/pnpm/pnpm/blob/31858c544b/installing/env-installer/src/parseIntegrity.ts).
#[derive(Debug, Clone)]
pub struct NormalizedSubdep {
    pub name: String,
    pub version: String,
    pub integrity: Integrity,
    pub tarball: String,
    pub os: Option<Vec<String>>,
    pub cpu: Option<Vec<String>>,
    pub libc: Option<Vec<String>>,
}

/// Split an old-format config-dependency value of the form
/// `<version>+<integrity>` into its parts. Mirrors pnpm's
/// [`parseIntegrity`](https://github.com/pnpm/pnpm/blob/31858c544b/installing/env-installer/src/parseIntegrity.ts):
/// the absence of a `+` separator means the user wrote a clean
/// specifier without committing an integrity, which is an error in the
/// migration path.
pub fn parse_integrity(
    pkg_name: &str,
    pkg_spec: &str,
) -> Result<(String, Integrity), ConfigDepError> {
    let Some((version, integrity)) = pkg_spec.split_once('+') else {
        return Err(ConfigDepError::NoIntegrity { name: pkg_name.to_string() });
    };
    let integrity = integrity
        .parse::<Integrity>()
        .map_err(|error| ConfigDepError::BadConfigDep {
            message: format!(
                r#"Config dependency "{pkg_name}" has an unparsable integrity "{integrity}": {error}"#,
            ),
        })?;
    Ok((version.to_string(), integrity))
}
