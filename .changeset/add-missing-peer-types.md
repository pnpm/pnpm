---
"pacquet": minor
---

Added the `addMissingPeerTypes` setting. When it is `true`, every peer dependency of a package also makes its `@types` package an optional peer. For example, a package that peer-depends on `react` gets the project's `@types/react` linked next to it. This lets TypeScript find the types of a peer dependency when the global virtual store is on [#15689](https://github.com/pnpm/pnpm/discussions/15689).
