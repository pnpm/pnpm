---
"@pnpm/resolver-base": minor
"@pnpm/git-resolver": minor
"@pnpm/prepare-package": minor
"@pnpm/git-fetcher": minor
"@pnpm/tarball-fetcher": minor
---

Add subfolder support for Git protocol using `path` parameter. For example, `pnpm i github:user/repo#path:packages/foo` will add a dependency from the subfolder `packages/foo`. It can work with the existing feature, e.g. `pnpm i github:user/repo#dev&path:packages/bar` will tell pnpm to use the subfolder `packages/bar` from branch `dev`.
