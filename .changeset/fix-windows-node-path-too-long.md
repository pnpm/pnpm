---
"@pnpm/link-bins": patch
"pnpm": patch
---

Fixed "input line too long" error on Windows when running lifecycle scripts with the global virtual store enabled. The `NODE_PATH` in command shims no longer includes redundant paths from `Module._nodeModulePaths()` — Node.js already searches those directories during standard `require()` resolution. Only the hoisted `node_modules` directory (from `extraNodePaths`) is included, which is the only path that standard resolution can't find on its own.
