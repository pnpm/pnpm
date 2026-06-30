---
"@pnpm/config.version-policy": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/installing.commands": minor
"@pnpm/exec.commands": minor
"@pnpm/cli.parse-cli-args": patch
"@pnpm/cli.common-cli-options-help": patch
"pnpm": minor
---

Added `--minimum-release-age` and `--minimum-release-age-exclude` CLI flags to `install`, `add`, `update`, and `dlx` commands. This allows setting `minimumReleaseAge` per-invocation without a config file [#11224](https://github.com/pnpm/pnpm/issues/11224).

Also hardened `getPublishedByPolicy()` so `minimumReleaseAge` rejects `NaN`, `Infinity`, and negative values (which previously disabled the supply-chain maturity gate or crashed on `toISOString()`), and a single `--minimum-release-age-exclude` flag value is coerced to an array so the exclude matcher works for the common single-value invocation. The same validation and coercion is applied in `createNpmResolutionVerifier()` so the lockfile verification path cannot be bypassed or crashed by the same invalid inputs.
