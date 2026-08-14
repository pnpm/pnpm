---
"@pnpm/bins.remover": patch
"@pnpm/global.commands": patch
"pnpm": patch
"pacquet": patch
---

Global installs now switch over atomically. The command shims in the global bin directory point at a stable per-package link rather than at the directory a particular install produced, so `pnpm add -g` and `pnpm update -g` activate a new version by moving that one link instead of rewriting every shim. A command can no longer be missing from `PATH` while an install is in progress, and a failed install leaves the previous version in place.
