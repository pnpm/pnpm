---
"pacquet": minor
---

With `nodeLinker.type: loaded`, packages that need a build no longer have to be listed in `nodeLinker.excluded`. A package with a matching side-effects cache entry loads from the store. When side-effects cache reads and writes are enabled, an approved build whose dependencies can load from the store runs in a temporary directory. If its output is compatible, it loads from the store too. Other cache misses, packages with native `.node` addons, and their dependencies are materialized automatically.

pnpm also scans JavaScript sources for filesystem imports and for `__dirname` or `import.meta`, and materializes matching packages. The scan does not detect computed imports. List such packages in `nodeLinker.excluded`.
