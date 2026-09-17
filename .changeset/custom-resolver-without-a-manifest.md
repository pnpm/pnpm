---
"pacquet": patch
---

A pnpmfile `resolvers` hook can return a resolution without a `manifest`. Such a package installed alone, with none of its own dependencies and no warning. pnpm now reads the manifest from the resolved tarball [#15000](https://github.com/pnpm/pnpm/issues/15000).
