---
"pacquet": patch
---

`pnpm licenses list` now reads packages from the directories where `nodeLinker: hoisted` placed them. It used to report paths under `node_modules/.pnpm` that do not exist in that layout, and listed every package's license as `Unknown` [#8589](https://github.com/pnpm/pnpm/issues/8589).
