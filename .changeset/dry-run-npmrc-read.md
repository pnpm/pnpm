---
"@pnpm/config.reader": patch
"pnpm": patch
---

Allow `dry-run` to be read from `.npmrc` files so that `pnpm pack --dry-run` works when configured via project or user configuration [#12054](https://github.com/pnpm/pnpm/issues/12054).
