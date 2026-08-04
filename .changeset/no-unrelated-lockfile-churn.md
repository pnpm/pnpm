---
"pacquet": patch
---

Fixed `pnpm install` rewriting unrelated `pnpm-lock.yaml` entries after a small manifest change — for example, removing one dev dependency could bump other packages' open-range dependencies (such as jest's `@types/node: '*'`) to their newest versions [pnpm/pnpm#13193](https://github.com/pnpm/pnpm/pull/13193). Three resolution-reuse gaps caused still-satisfied lockfile entries to be re-resolved from the registry:

- Direct dependencies using the `catalog:` protocol were compared against the lockfile in their resolved-range form, so every catalog-managed dependency looked changed on every install, and any package depending on one was re-resolved.
- Auto-installed (hoisted) peer dependencies were also treated as changed direct dependencies on every install.
- When a package had to resolve freshly but landed on the version the lockfile already recorded, its dependency subtree was still re-resolved instead of being reused, drifting open ranges pinned by the lockfile.
