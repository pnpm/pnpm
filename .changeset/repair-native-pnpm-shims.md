---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

pnpm 11 now runs projects pinned to pnpm 12.3 or later without passing the native pnpm binary to Node.js. Cached installations with stale launchers are repaired automatically [#14502](https://github.com/pnpm/pnpm/issues/14502).
