---
"@pnpm/resolving.npm-resolver": patch
"pacquet": patch
"pnpm": patch
---

Installs are faster in workspaces that declare inter-workspace dependencies with plain ranges (`"*"`, `"^1.2.3"`) rather than the `workspace:` protocol. With `preferWorkspacePackages` enabled, the registry request made before linking such a dependency could not change the outcome, and workspace packages that were never published returned an uncached 404 on every install. That request is now skipped when exactly one workspace copy carries the name and satisfies the range. The registry is still consulted for injected dependencies, when several workspace copies share a name, under `trustPolicy=no-downgrade`, and when `updateChecksums` is set.
