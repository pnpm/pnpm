---
"@pnpm/deps.compliance.commands": minor
"@pnpm/config.writer": minor
"@pnpm/workspace.workspace-manifest-writer": minor
"pnpm": minor
---

`pnpm audit --fix` now adds the minimum patched versions to `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` [#10263](https://github.com/pnpm/pnpm/issues/10263).

When `minimumReleaseAge` is configured, security patches suggested by `pnpm audit` may be blocked because the patched versions are too new. Now, `pnpm audit --fix` automatically adds the minimum patched version for each vulnerability (e.g., `axios@0.21.2`) to `minimumReleaseAgeExclude`, so that `pnpm install` can install the security fix without waiting for it to mature.
