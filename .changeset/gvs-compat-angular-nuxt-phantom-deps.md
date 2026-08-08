---
"@pnpm/hooks.read-package-hook": patch
"pnpm": patch
"pacquet": patch
---

Added compatibility `packageExtensions` for `@angular/build` (undeclared `tslib`) and `@nuxt/vite-builder@>=4` (undeclared `unplugin`), so `ng build` and `nuxt build` work under the global virtual store, where undeclared dependencies are not resolvable by design.
