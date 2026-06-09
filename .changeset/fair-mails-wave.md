---
"@pnpm/deps.graph-builder": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

Ensure `pnpm fetch` still applies patches to registry packages that are only reachable through skipped local `file:` dependencies.
