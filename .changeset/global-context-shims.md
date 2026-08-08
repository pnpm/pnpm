---
"pacquet": minor
---

Globally installed bins (`pnpm add -g`, `pnpm runtime set node -g`) are now context-aware shims. Running `node` — or any globally installed tool — inside a project runs the project's own version: the shim walks up from the current directory and prefers the project's `node_modules/.bin/<name>` over the global install, and when a project pins Node.js in `devEngines.runtime` but has no installed `node_modules` yet, the pinned version is downloaded on demand and run. The first time a project's binaries are used this way, pnpm asks "Do you trust this project?" and remembers the answer on the machine (outside the project). Non-interactive sessions fall back to the global version. Set `globalShims: false` to restore the previous direct shims, or set `PNPM_SHIM_BYPASS=1` to skip the project lookup for a single invocation. On Windows, the global `node.exe` keeps its previous format for compatibility with tools that spawn `node` directly.
