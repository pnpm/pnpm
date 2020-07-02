---
"@pnpm/config": minor
"@pnpm/plugin-commands-installation": major
---

A new setting is returned by `@pnpm/config`: `npmGlobalBinDir`.
`npmGlobalBinDir` is the global executable directory used by npm.

This new config is used by `@pnpm/global-bin-dir` to find a suitable
directory for the binstubs installed by pnpm globally.
