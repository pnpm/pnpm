---
"@pnpm/config.reader": patch
"pnpm": patch
---

Validate the deprecated updateConfig setting and its nested fields (ignoreDependencies, changeset, githubActions, githubActionsServer) when present in workspace or global configuration.
