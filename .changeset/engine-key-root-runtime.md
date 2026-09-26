---
"@pnpm/deps.graph-hasher": patch
"@pnpm/deps.graph-builder": patch
"@pnpm/building.after-install": patch
"@pnpm/building.during-install": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

The global virtual store and the side-effects cache now key built packages by the Node.js version that the root project's `devEngines.runtime` or `engines.runtime` pins. That is the Node.js their build scripts run with. A dependency that declares its own `engines.runtime` no longer changes the key for every other package.
