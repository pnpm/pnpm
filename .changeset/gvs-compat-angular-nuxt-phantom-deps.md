---
"@pnpm/hooks.read-package-hook": patch
"pnpm": patch
"pacquet": patch
---

`ng build` and `nuxt build` now work under the global virtual store: pnpm's built-in compatibility extensions add the `tslib` dependency that `@angular/build` uses without declaring and the `unplugin` dependency that `@nuxt/vite-builder` v4 uses without declaring.
