---
"pacquet": patch
---

`pnpm sbom` now selects the workspace projects it covers through the same filtering machinery as every other filterable command, so `--filter-prod` is honoured and `--filter` accepts the full selector syntax (dependency queries such as `pkg...`, `{dir}` and glob paths, `[since]` change queries, exclusions) rather than matching selectors against lockfile importer ids. `--workspace-root` narrows the SBOM to the root project, and selectors that match no project print pnpm's `No projects matched the filters` notice instead of writing an SBOM of nothing.

The universal `--fail-if-no-match` flag is supported too: any filtered command whose selectors match no workspace project now exits with code 1 [#14064](https://github.com/pnpm/pnpm/issues/14064).
