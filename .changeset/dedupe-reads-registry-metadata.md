---
"pacquet": patch
---

`pnpm dedupe` now reads registry metadata for a dependency pinned to an exact version, as `pnpm install` does. If the registry metadata disagreed with the package's `package.json`, the lockfile it wrote depended on whether `minimumReleaseAge` was set [#16615](https://github.com/pnpm/pnpm/issues/16615).
