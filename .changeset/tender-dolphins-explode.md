---
"@pnpm/resolver-base": minor
"@pnpm/git-resolver": minor
"@pnpm/prepare-package": minor
"@pnpm/git-fetcher": minor
"@pnpm/tarball-fetcher": minor
"pnpm": minor
---

It is now possible to install only a subdirectory from a Git repository.

For example, `pnpm add github:user/repo#path:packages/foo` will add a dependency from the `packages/foo` subdirectory.

This new parameter may be combined with other supported parameters separated by `&`. For instance, the next command will install the same package from the `dev` branch: `pnpm add github:user/repo#dev&path:packages/bar`.

Related issue: [#4765](https://github.com/pnpm/pnpm/issues/4765).
Related PR: [#7487](https://github.com/pnpm/pnpm/pull/7487).
