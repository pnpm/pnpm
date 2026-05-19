---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fix `config.registry` getting a trailing slash appended when `registry` is set in `.npmrc` and no `registries.default` is provided by `pnpm-workspace.yaml`. The sync from `registries.default` to `config.registry` introduced in #11744 now only fires when the workspace manifest actually contributes a different default.
