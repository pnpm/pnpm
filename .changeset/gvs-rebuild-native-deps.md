---
"@pnpm/building.after-install": patch
---

Fix dependency build scripts not running under the global virtual store (`enableGlobalVirtualStore`).

In a workspace install, dependency build scripts are deferred to a single `rebuild` pass (`buildProjects`). That pass resolved each package's location from the classic `node_modules/.pnpm/<depPathToFilename>` layout, which does not exist under the global virtual store — so native dependencies (e.g. packages using `node-gyp` / `prebuild-install`) were never built and failed to load at runtime (`Cannot find module .../build/Release/*.node`).

`buildProjects` now resolves the global-virtual-store projection directory (`<storeDir>/links/<hash>`, computed with the same graph hash the installer uses) when `enableGlobalVirtualStore` is set, and serializes concurrent builds of the same shared projection so parallel workspace projects don't race on the same directory.
