---
"pacquet": patch
---

With `nodeLinker: hoisted`, `pnpm licenses list` reported every license as `Unknown` and listed paths under `node_modules/.pnpm` that do not exist. It now reads each package from the directory where the hoisted linker placed it [#8589](https://github.com/pnpm/pnpm/issues/8589).
