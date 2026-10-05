---
"@pnpm/exec.lifecycle": patch
"pnpm": patch
"pacquet": patch
---

Dependency lifecycle scripts now use the standard platform shell. The `scriptShell` setting continues to apply to project scripts.
