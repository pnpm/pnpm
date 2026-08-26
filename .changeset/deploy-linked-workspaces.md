---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now creates its dedicated lockfile from linked workspace dependencies when `injectWorkspacePackages` is disabled. Docker builds can use native, frozen-lockfile deploys without changing how workspace packages are linked during local installs or enabling the legacy deploy implementation.
