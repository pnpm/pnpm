---
"pacquet": patch
---

With `injectWorkspacePackages: true`, a fresh `pnpm install` now records a workspace dependency as `link:` when its injected copy differs from the project only by an optional peer that peer-dependent dedupe merges. It was recorded as a peer-suffixed `file:` copy [#16354](https://github.com/pnpm/pnpm/issues/16354).
