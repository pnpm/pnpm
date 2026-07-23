---
"@pnpm/deps.inspection.commands": minor
"@pnpm/installing.commands": minor
"@pnpm/config.reader": minor
"@pnpm/resolving.git-resolver": patch
"@pnpm/types": minor
"pnpm": minor
"pacquet": minor
---

Added GitHub Actions dependencies to `pnpm outdated` and `pnpm update`. Both commands include them when `--include-github-actions` is passed or `update.githubActions` is set to `true` in `pnpm-workspace.yaml`. Updated actions are pinned to exact commit hashes with their release tags preserved in comments.
