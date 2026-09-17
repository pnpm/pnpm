---
"pacquet": patch
---

pnpm now reads the manifest from the tarball when a pnpmfile `resolvers` hook returns a resolution without one. Such a package installed alone, with none of its own dependencies and no warning [#15000](https://github.com/pnpm/pnpm/issues/15000).
