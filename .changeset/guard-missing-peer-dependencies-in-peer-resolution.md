---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm deploy --legacy` no longer fails with `Cannot convert undefined or null to object` when the dependency graph contains a package whose resolved manifest carries no `peerDependencies` field.
