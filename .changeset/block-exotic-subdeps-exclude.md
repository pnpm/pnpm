---
"@pnpm/config.reader": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": minor
"pnpm": minor
---

Added a new setting `blockExoticSubdepsExclude`, a list of trusted sources (git repositories or tarball/URL hosts) that are allowed to be installed as exotic subdependencies even when `blockExoticSubdeps` is enabled. Entries are matched against the normalized source URL of the dependency (e.g. `https://github.com/user/repo`) and support `*` wildcards, so a whole host or organization can be trusted at once (e.g. `https://github.com/my-org/*`).
