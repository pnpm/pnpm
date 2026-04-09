---
"@pnpm/installing.commands": minor
"@pnpm/exec.commands": minor
"@pnpm/cli.parse-cli-args": patch
"pnpm": minor
---

Added `--minimum-release-age` and `--minimum-release-age-exclude` CLI flags to `install`, `add`, `update`, and `dlx` commands. This allows setting `minimumReleaseAge` per-invocation without a config file [#11224](https://github.com/pnpm/pnpm/issues/11224).
