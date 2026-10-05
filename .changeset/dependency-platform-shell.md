---
"@pnpm/exec.lifecycle": patch
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
"pacquet": patch
---

Dependency lifecycle scripts now use the standard platform shell. The `scriptShell` setting continues to apply to project scripts. Scripts can run when `PATH` contains only a downloaded runtime.
