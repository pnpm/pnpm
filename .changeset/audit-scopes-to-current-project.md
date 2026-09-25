---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit` run from a workspace project now checks only the dependencies of that project. It used to audit every project in the workspace, so the report included vulnerabilities from the root manifest and from sibling projects. `pnpm -r audit` and a `--filter` selector still cover the whole workspace or the selected projects [#8358](https://github.com/pnpm/pnpm/issues/8358).

`pnpm audit --fix` with `audit.ignorePrune` now leaves `auditConfig.ignoreGhsas` unchanged when the audit covers only some of the workspace projects, and prints a warning. Pruning from a partial report used to remove ignores that the root or a sibling project still needs.
