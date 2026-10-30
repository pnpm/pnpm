---
"pacquet": minor
---

With `nodeLinker.type: loaded`, packages that need a build no longer have to be listed in `nodeLinker.excluded`. A package with a matching side-effects cache entry loads from the store. If side-effects cache reads and writes are enabled, pnpm builds an approved package once, saves the result to the side-effects cache, and then loads it from the store. This includes a build that changes no files in its package, so its script does not run again in other projects that use the same store. List a package in `sideEffectsCache.exclude` if its build must run in every project, such as a git hook installer. Other cache misses, packages with native `.node` addons, and their dependencies are materialized automatically.

pnpm also scans JavaScript sources for filesystem imports and for `__dirname` or `import.meta`, and materializes matching packages. The scan does not detect computed imports. List such packages in `nodeLinker.excluded`.
