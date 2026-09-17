---
"pacquet": patch
---

Fixed a pnpmfile `resolvers` hook that returns no `manifest` installing the package without its dependencies [#15000](https://github.com/pnpm/pnpm/issues/15000). `manifest` is optional in the hook's result, and pnpm now reads it from the resolved tarball.
