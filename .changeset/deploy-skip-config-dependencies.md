---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fix `pnpm deploy` crashing with `ENOENT: ... lstat '<deployDir>/node_modules'` when `configDependencies` declares pacquet (`pacquet` or `@pnpm/pacquet`). The deploy directory never installs config dependencies, so the install engine they designate isn't on disk to invoke; the nested install now skips them.
