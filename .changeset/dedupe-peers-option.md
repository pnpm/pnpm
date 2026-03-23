---
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/lockfile.types": minor
"@pnpm/lockfile.settings-checker": minor
"@pnpm/config.reader": minor
"pnpm": minor
---

Added a new `dedupePeers` setting that reduces peer dependency duplication by using per-project peer deduplication. When enabled, peer dependency suffixes use version-only identifiers (no nested dep paths) and transitive peer propagation is stopped — only directly declared peers appear in a package's suffix. This dramatically reduces the number of package instances in projects with many recursive peer dependencies [#11070](https://github.com/pnpm/pnpm/issues/11070).
