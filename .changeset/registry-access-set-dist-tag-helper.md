---
"@pnpm/registry-access.commands": minor
---

Expose `setDistTag` — a low-level helper that PUTs a dist-tag to a registry. The CLI `dist-tag add` handler is refactored to call it, and downstream tests in this monorepo use it (via the new `@pnpm/testing.registry-mock` wrapper) instead of the legacy `addDistTag` from `@pnpm/registry-mock`, which relied on the unmaintained `anonymous-npm-registry-client` and a verdaccio-era fetch-then-delete-then-add workaround.
