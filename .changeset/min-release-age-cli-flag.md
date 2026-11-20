---
"@pnpm/cli.common-cli-options-help": patch
"@pnpm/config.reader": patch
"@pnpm/config.version-policy": patch
"@pnpm/exec.commands": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": minor
---

Added the `--minimum-release-age` and `--minimum-release-age-exclude` options to `pnpm install`, `pnpm add`, `pnpm update`, and `pnpm dlx`. They set `minimumReleaseAge` and `minimumReleaseAgeExclude` for a single run without editing a config file [#11224](https://github.com/pnpm/pnpm/issues/11224).
