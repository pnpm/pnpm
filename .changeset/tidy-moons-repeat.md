---
"@pnpm/napi": minor
---

Added the `returnListOfDepsRequiringBuild` install option. When set, `InstallResult.depsRequiringBuild` lists the dep path of every package whose files carry install scripts — whether or not the scripts were allowed to run — matching the TypeScript engine's option of the same name. The list is computed only when a fresh resolve materializes `node_modules`; an install served from the frozen-lockfile path leaves the field undefined so embedders keep their previously recorded list.
