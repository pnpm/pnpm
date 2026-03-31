---
"@pnpm/deps.compliance.license-checker": minor
"@pnpm/deps.compliance.commands": minor
"@pnpm/types": minor
"@pnpm/workspace.workspace-manifest-reader": minor
"@pnpm/config.reader": patch
"@pnpm/installing.commands": minor
"pnpm": minor
---

Added built-in license compliance auditing via `licenses` in `pnpm-workspace.yaml`. New subcommands: `pnpm licenses check`, `pnpm licenses allow`, `pnpm licenses disallow` [#10570](https://github.com/pnpm/pnpm/issues/10570).
