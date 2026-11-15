---
"pacquet": patch
---

`pnpm install` no longer runs `git ls-remote` for a git dependency whose specifier is unchanged and whose commit the lockfile already records. Installs that update the lockfile, such as Renovate's, no longer fail without git credentials when the project has a private git dependency with peer dependencies [#18104](https://github.com/pnpm/pnpm/issues/18104).
