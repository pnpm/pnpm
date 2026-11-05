---
"pacquet": minor
---

Added the `projectScopedOptionalPeers` setting. When it is `true`, a project gets a missing optional peer dependency only if the project's own dependencies install a version of it. Another workspace project's dependencies, such as its `devDependencies`, no longer supply one. `dedupePeerDependents` keeps deduplicating everything else [pnpm/pnpm#16666](https://github.com/orgs/pnpm/discussions/16666).
