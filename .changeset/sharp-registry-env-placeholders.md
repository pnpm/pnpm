---
"@pnpm/config.reader": minor
"pnpm": minor
---

Stopped expanding environment variables in repository-controlled registry/proxy request destinations and registry credential values from `.npmrc`, and in workspace registry URLs from `pnpm-workspace.yaml`. Move dynamic registry URL and token configuration to trusted user, global, CLI, or environment config.
