---
"@pnpm/store-connection-manager": minor
"@pnpm/tarball-fetcher": minor
"@pnpm/npm-resolver": minor
"@pnpm/client": minor
"@pnpm/config": minor
"pnpm": minor
---

Added network performance monitoring to pnpm by implementing warnings for slow network requests, including both metadata fetches and tarball downloads.

Added configuration options for warning thresholds: `fetchWarnTimeoutMs` and `fetchMinSpeedKiBps`.
Warning messages are displayed when requests exceed time thresholds or fall below speed minimums

Related PR: [#10025](https://github.com/pnpm/pnpm/pull/10025).

