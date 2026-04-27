---
"pnpm": patch
---

Remove pnpm's workspace state file when cleaning node_modules so `pnpm ci` performs a fresh install after the clean step.
