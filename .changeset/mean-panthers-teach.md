---
"@pnpm/create-cafs-store": patch
"@pnpm/headless": patch
"@pnpm/core": patch
"pnpm": patch
---

Optional dependencies that do not have to be built will be reflinked (or hardlinked) to the store instead of copied [#7046](https://github.com/pnpm/pnpm/issues/7046).
