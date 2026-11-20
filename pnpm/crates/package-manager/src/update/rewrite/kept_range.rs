use crate::package_manifest_prefix;
use node_semver::Version;
use pnpm_reporter::{
    LogEvent,
    LogLevel,
    PnpmLog,
};
use pnpm_resolving_resolver_base::parse_strict_semver_range;

use super::{
    LatestRewriteCtx,
    MatchedRewrite,
};

/// What an update that does not save may do with a requested specifier, given
/// the specifier the manifest keeps.
pub(in super::super) enum KeptRangeVerdict {
    /// A version the kept range admits: resolution can be pointed at it.
    Admitted,
    /// A version the kept range excludes: the dependency is left alone.
    Excluded,
    /// Nothing that can be judged before resolution, such as a range or a dist
    /// tag, or a kept specifier that is not a semver range. The kept specifier
    /// decides.
    Undecided,
}

/// The declaration an update that does not save may write: only a version the
/// kept range already admits.
pub(in super::super) fn kept_range_rewrite<Reporter: pnpm_reporter::Reporter>(
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    name: &str,
    requested: &str,
    previous: &str,
) -> MatchedRewrite {
    match judge_against_kept_range(requested, previous) {
        KeptRangeVerdict::Admitted => MatchedRewrite::Target(Some(requested.to_string())),
        KeptRangeVerdict::Excluded => {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    r#"Skipping "{name}@{requested}": it doesn't satisfy "{previous}", which the manifest keeps when updating without saving."#,
                ),
                prefix: package_manifest_prefix(rewrite_ctx.manifest),
            }));
            MatchedRewrite::Skipped
        }
        KeptRangeVerdict::Undecided => {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    r#"Ignoring "{name}@{requested}": the manifest keeps "{previous}" when updating without saving, so "{name}" was updated within that range instead."#,
                ),
                prefix: package_manifest_prefix(rewrite_ctx.manifest),
            }));
            MatchedRewrite::Target(None)
        }
    }
}

/// Judge a requested specifier against the range the manifest keeps.
///
/// Only a concrete version gets a verdict. Matching a version against a range
/// is exact; deciding whether one *range* is contained by another is not, so a
/// range is left [`Undecided`] rather than guessed at.
///
/// [`Undecided`]: KeptRangeVerdict::Undecided
pub(in super::super) fn judge_against_kept_range(requested: &str, kept: &str) -> KeptRangeVerdict {
    let (Ok(requested), Some(kept)) = (Version::parse(requested), parse_strict_semver_range(kept))
    else {
        return KeptRangeVerdict::Undecided;
    };
    if requested.satisfies(&kept) { KeptRangeVerdict::Admitted } else { KeptRangeVerdict::Excluded }
}
