---
"@pnpm/config.reader": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": minor
"pnpm": minor
---

Added a new setting `blockExoticSubdepsExclude`, a list of trusted packages that are allowed to be installed as exotic (e.g. git-hosted) subdependencies even when `blockExoticSubdeps` is enabled. Entries are matched by package name (the alias used in the dependency tree, falling back to the resolved package name) and support wildcards (e.g. `@scope/*`) and exact-version pins (e.g. `foo@1.0.0`).
