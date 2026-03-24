---
"@pnpm/resolve-dependencies": minor
"@pnpm/core": minor
"@pnpm/lockfile.types": minor
"@pnpm/lockfile.settings-checker": minor
"@pnpm/config": minor
"pnpm": minor
---

Added a new `dedupePeers` setting that reduces peer dependency duplication. When enabled, peer dependency suffixes use version-only identifiers (`name@version`) instead of full dep paths, eliminating nested suffixes like `(foo@1.0.0(bar@2.0.0))`. This dramatically reduces the number of package instances in projects with many recursive peer dependencies [#11070](https://github.com/pnpm/pnpm/issues/11070).
