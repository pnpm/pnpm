---
"pacquet": patch
---

`pnpm sbom` now honours `--filter-prod`, the full `--filter` selector syntax (dependency queries such as `pkg...`, `{dir}` and glob paths, `[since]` change queries, exclusions), and `--workspace-root`. Selectors that match no project print `No projects matched the filters` and write no SBOM, and `--split` emits its per-project SBOMs in a stable order.

The universal `--fail-if-no-match` flag is supported too: any filtered command whose selectors match no workspace project now exits with code 1 [#14064](https://github.com/pnpm/pnpm/issues/14064).
