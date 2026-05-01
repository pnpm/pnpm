---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fix recursive publish summaries to report the manifest from `publishConfig.directory` when packages publish from a generated directory [#11239](https://github.com/pnpm/pnpm/issues/11239).
