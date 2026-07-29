---
"pnpm": patch
---

Fixed catalog updates with explicit dist tags, so commands like `pnpm update package@beta` keep the dependency using `catalog:` in `package.json` and update the catalog entry instead of writing the resolved specifier directly to the project manifest.
