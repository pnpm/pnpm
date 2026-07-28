---
"@pnpm/resolving.git-resolver": patch
"pnpm": patch
---

`pnpm outdated --include-github-actions` no longer blocks on an interactive git credential prompt when a workflow uses a private action repo.
