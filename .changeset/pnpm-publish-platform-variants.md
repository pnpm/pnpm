---
"pacquet": minor
---

`pnpm publish` publishes a package's `publishConfig.variants` to a registry declared with `serverType: pnpr`. pnpm uploads every archive before the publish request that references them. Publishing variants to any other registry is refused unless `--no-variants` is given, which publishes only the archive that targets `any`.
