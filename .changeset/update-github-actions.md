---
"@pnpm/deps.github-actions": minor
"@pnpm/deps.inspection.commands": minor
"@pnpm/installing.commands": minor
"@pnpm/config.reader": minor
"@pnpm/resolving.git-resolver": patch
"@pnpm/types": minor
"pnpm": minor
"pacquet": minor
---

Added GitHub Actions dependencies to `pnpm outdated` and interactive `pnpm update`. Non-interactive updates can include them with `--include-github-actions` or by setting `update.githubActions` to `true` in `pnpm-workspace.yaml`. Updated actions are pinned to exact commit hashes with their release tags preserved in comments.
