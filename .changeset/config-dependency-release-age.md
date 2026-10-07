---
"@pnpm/config.version-policy": patch
"@pnpm/installing.env-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` and `pnpm add --config` now apply `minimumReleaseAge` when they resolve a config dependency. A config dependency range resolves to the newest version that is old enough, so a later clean `pnpm install --frozen-lockfile` accepts the lockfile [#16660](https://github.com/pnpm/pnpm/issues/16660).
