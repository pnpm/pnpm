---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm install --dry-run` no longer runs the root project's `pnpm:devPreinstall` script.
