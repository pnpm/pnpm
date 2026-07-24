---
"@pnpm/releasing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm version -r --json` now outputs `[]` instead of human-readable text when no pending changes exist [`pnpm/pnpm#13217`](https://github.com/pnpm/pnpm/issues/13217).
