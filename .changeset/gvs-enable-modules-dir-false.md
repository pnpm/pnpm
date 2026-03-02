---
"@pnpm/headless": patch
"pnpm": patch
---

Fixed a bug where `enableModulesDir: false` prevented the Global Virtual Store `links/` directory from being populated. The `links/` directory is part of the store, not the project's `node_modules/`, so it should be written regardless of the `enableModulesDir` setting when GVS is enabled.
