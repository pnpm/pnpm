---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fixed an issue where running `pnpm dedupe --check` in projects with `nodeLinker: hoisted` would cause dependencies to be moved out of `node_modules` into `node_modules/.ignored`.
