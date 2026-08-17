---
"@pnpm/resolving.npm-resolver": patch
"pacquet": patch
"pnpm": patch
---

Installs are faster in workspaces that declare inter-workspace dependencies with plain ranges (`"*"`, `"^1.2.3"`) rather than the `workspace:` protocol. With `preferWorkspacePackages` enabled, linking such a dependency no longer makes a registry request that cannot change the outcome — and workspace packages that were never published no longer cost a 404 on every install.
