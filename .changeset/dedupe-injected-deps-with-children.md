---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fix `link:` workspace protocol switching to `file:` after `pnpm rm` is run from inside a workspace package whose target workspace dependency has its own dependencies, when `injectWorkspacePackages: true` is set. Follow-up to [#10575](https://github.com/pnpm/pnpm/pull/10575), which fixed the same symptom for workspace packages without dependencies.
