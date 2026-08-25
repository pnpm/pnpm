---
"pacquet": patch
---

A repeat `pnpm install` with `nodeLinker: hoisted` is a no-op again when a workspace package declares the dependencies [#14001](https://github.com/pnpm/pnpm/issues/14001). The hoisted linker installs them into the root `node_modules`, but the up-to-date check previously looked under each package's own `node_modules` and reinstalled the whole tree every time. A hoisted install also no longer reports the packages it just wrote as broken.
