---
"pacquet": patch
---

Suggest `pnpm shim add <runtime>` after pinning a project runtime when no project-aware global shim is installed. Explicit project-aware shims now reject unrelated global bin conflicts and are restored after a matching global package is removed.
