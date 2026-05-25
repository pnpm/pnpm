---
"@pnpm/registry-access.commands": patch
---

Refactor the CLI `dist-tag add` handler to delegate the PUT to the new `@pnpm/registry-access.set-dist-tag` package — a tiny shared helper that downstream tests in this monorepo also use (via `@pnpm/testing.registry-mock`) instead of the legacy `addDistTag` from `@pnpm/registry-mock`, which relied on the unmaintained `anonymous-npm-registry-client` and a verdaccio-era fetch-then-delete-then-add workaround.
