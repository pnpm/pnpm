---
"pacquet": minor
---

Globally installed Node.js runtimes now follow stable project runtime pins by default. pnpm authenticates the release and executes it from the trusted global virtual store without consulting the project's `node_modules/.bin`. Set `globalShims: all` to opt into prompted project-local switching for ordinary package bins, `globalShims: off` to disable switching, or `PNPM_SHIM_BYPASS=1` to bypass it once.
