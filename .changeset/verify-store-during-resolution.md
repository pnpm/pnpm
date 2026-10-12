---
"pacquet": patch
"@pnpm/napi": patch
---

An install that fetches into the store without writing `node_modules` (`enableModulesDir: false`) now verifies the packages the store already holds while it resolves the dependencies, instead of after the resolution. The verification no longer stats the files of a large package one after another either. On a store on a network filesystem this removes most of the time such an install spent after resolving.
