---
"@pnpm/releasing.commands": patch
"@pnpm/releasing.versioning": patch
"pnpm": patch
"pacquet": patch
---

`pnpm version` now applies pending bumps to private workspace packages. A private package's changelog is written to its committed `CHANGELOG.md`, also when `versioning.changelog.storage` is `registry`
[pnpm/pnpm#13736](https://github.com/pnpm/pnpm/issues/13736),
[pnpm/pnpm#13519](https://github.com/pnpm/pnpm/issues/13519).
