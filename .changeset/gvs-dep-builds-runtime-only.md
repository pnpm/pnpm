---
"@pnpm/bins.linker": patch
"@pnpm/building.during-install": patch
"@pnpm/building.after-install": patch
"pnpm": patch
---

With `enableGlobalVirtualStore`, dependency build scripts no longer see the bins of the workspace root or of hoisted dependencies. They still find the Node.js that the root project's `devEngines.runtime` or `engines.runtime` installs.
