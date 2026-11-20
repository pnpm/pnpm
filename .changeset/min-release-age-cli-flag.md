---
"@pnpm/config.version-policy": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/installing.commands": minor
"@pnpm/exec.commands": minor
"@pnpm/cli.common-cli-options-help": patch
"pnpm": minor
"pacquet": minor
---

Added the `--minimum-release-age` and `--minimum-release-age-exclude` CLI flags to `pnpm install`, `pnpm add`, `pnpm update`, and `pnpm dlx`. They set `minimumReleaseAge` and `minimumReleaseAgeExclude` for a single run without editing a config file [#11224](https://github.com/pnpm/pnpm/issues/11224).

`minimumReleaseAge` must now be a finite, non-negative number of minutes. Other values fail with `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE`, where they used to disable the check or crash the install.
