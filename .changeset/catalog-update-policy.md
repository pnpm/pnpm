---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Fixed `pnpm update` overriding the version range policy of a named catalog whose name parses as a version (e.g. `catalog:express4-21`). The `catalog:` reference carries no pinning of its own, so the prefix from the catalog entry (such as `~`) is now preserved instead of being widened to `^` [#10321](https://github.com/pnpm/pnpm/issues/10321).
