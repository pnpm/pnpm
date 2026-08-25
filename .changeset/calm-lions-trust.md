---
"@pnpm/cli-utils": patch
"@pnpm/config": patch
"@pnpm/tools.plugin-commands-self-updater": patch
"pnpm": patch
---

`pnpm self-update` now resolves and verifies pnpm through registry, authentication, proxy, and TLS settings from trusted non-project configuration. Project configuration and the default project pnpmfile can no longer redirect the pnpm download or disable engine identity verification.
