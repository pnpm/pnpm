---
"pacquet": patch
---

`pnpm install`, `pnpm add`, and `pnpm dedupe` now apply `ignoredOptionalDependencies`. Matching optional dependencies are left out of the lockfile and are not installed. pnpm 12 installed them whenever it resolved dependencies from scratch [pnpm/pnpm#14729](https://github.com/pnpm/pnpm/issues/14729).
