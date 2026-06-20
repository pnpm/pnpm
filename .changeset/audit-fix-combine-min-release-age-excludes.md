---
"@pnpm/workspace.workspace-manifest-writer": patch
"@pnpm/deps.compliance.commands": patch
"@pnpm/config.version-policy": patch
"pnpm": patch
---

`pnpm audit --fix` now writes a single combined `minimumReleaseAgeExclude` entry per package (e.g. `axios@0.18.1 || 0.21.1`) instead of one entry per version, matching the format documented for the setting. Existing per-version entries in `pnpm-workspace.yaml` are merged into the combined form rather than left as duplicates [#12534](https://github.com/pnpm/pnpm/issues/12534).
