---
"@pnpm/deps.status": patch
"@pnpm/fetching.directory-fetcher": patch
"@pnpm/fs.packlist": patch
"@pnpm/hooks.read-package-hook": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

Relative local paths in root `overrides` now resolve from the workspace root when each project has its own lockfile [#17691](https://github.com/pnpm/pnpm/issues/17691).

Installing a local directory dependency no longer fails when another project's install is writing a lockfile in that directory.
