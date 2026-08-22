---
"@pnpm/cli.commands": patch
"@pnpm/deps.status": patch
"@pnpm/workspace.projects-filter": patch
"pnpm": patch
---

Fixed workspace discovery for `pnpm-workspace.yaml` files without a `packages` field so commands only consider the workspace root instead of recursively scanning nested projects [#14047](https://github.com/pnpm/pnpm/issues/14047).
