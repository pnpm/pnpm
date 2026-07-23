---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed empty `bundledDependencies` and `bundleDependencies` arrays causing nondeterministic lockfile changes. See pnpm/pnpm#13123.
