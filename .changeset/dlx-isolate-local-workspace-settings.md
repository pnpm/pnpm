---
"pacquet": patch
---

`pnpm dlx`, `pnx`, and `pnpm create` now ignore untrusted execution, layout, and resolution settings in a local `pnpm-workspace.yaml`, including `pnpmfile`, `globalPnpmfile`, `configDependencies`, `scriptShell`, `cacheDir`, `storeDir`, and `ignoreScripts`. Settings from the user's global `config.yaml`, such as `blockExoticSubdeps`, `cacheDir`, and `storeDir`, remain active.
