---
"@pnpm/bins.resolver": patch
"pnpm": patch
---

pnpm skips unsupported command names in `directories.bin`, including filenames with spaces.
