---
"@pnpm/resolving.npm-resolver": patch
---

`preferWorkspacePackages` no longer pays a registry round-trip per workspace package name. When that setting is on and exactly one workspace copy carries the wanted name and satisfies the range, the registry response cannot change which package is chosen, so the request is now skipped entirely. Workspaces that declare inter-workspace dependencies with plain ranges (`"*"`, `"^1.2.3"`) rather than the `workspace:` protocol paid this on every install, and workspace packages that were never published paid a 404 each time because negative results are not cached. The request is still made for injected dependencies, when several workspace copies share a name, under `trustPolicy=no-downgrade`, and when `updateChecksums` is set.
