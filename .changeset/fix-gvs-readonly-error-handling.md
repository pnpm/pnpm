---
"@pnpm/installing.deps-restorer": patch
"@pnpm/bins.linker": patch
"pnpm": patch
---

Scope GVS read-only error suppression to Global Virtual Store code paths only. Non-GVS installs now correctly throw on permission errors instead of silently skipping bin linking and self-dep symlinking.
