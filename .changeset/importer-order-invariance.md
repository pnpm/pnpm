---
"pacquet": patch
---

The resolved dependency graph and lockfile no longer depend on the order in which workspace projects are listed or discovered: importers are processed in project-id order, so reordering the `packages` globs in `pnpm-workspace.yaml` (or any other change to project listing order) produces a byte-identical lockfile [#13846](https://github.com/pnpm/pnpm/issues/13846). This also makes auto-installed peer placement, deprecation-warning attribution, and cycle back-edge bindings a function of the project set alone.
