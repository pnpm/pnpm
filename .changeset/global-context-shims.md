---
"pacquet": minor
---

Globally installed bins (`pnpm add -g`, `pnpm runtime set node -g`) are now context-aware shims. Inside a project, a bare command such as `node`, `tsc`, or `eslint` prefers the matching project-local package; pinned runtimes can be downloaded on demand. pnpm asks before using a project bin, remembers the decision for that exact provider and dependency state, and falls back to the global command in non-interactive sessions. Set `globalShims: false` to disable the behavior or `PNPM_SHIM_BYPASS=1` to bypass it once. On Windows, the global `node.exe` keeps its direct-executable format.
