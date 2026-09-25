---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now prints collected config warnings when config loading fails. An auth credential with an unresolved environment variable placeholder now reports a specific hint [pnpm/pnpm#11298](https://github.com/pnpm/pnpm/issues/11298).
