---
"pacquet": patch
---

`pnpm install` now runs every hook a config dependency plugin's pnpmfile exports, including `readPackage`, `afterAllResolved`, and custom resolvers. Its pnpmfile is also counted in `pnpmfileChecksum`. Before, only the plugin's `updateConfig` hook ran, so a plugin could not change the resolved dependencies.
