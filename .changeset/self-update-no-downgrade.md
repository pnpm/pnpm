---
"@pnpm/engine.pm.commands": patch
"pnpm": minor
---

`pnpm self-update` (with no version argument) no longer downgrades pnpm when the registry's `latest` dist-tag points to an older release than the currently active version. Run `pnpm self-update latest` to force a downgrade [#11418](https://github.com/pnpm/pnpm/issues/11418).
