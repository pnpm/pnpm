---
"@pnpm/plugin-commands-deploy": patch
---

Removed a defunct special case to handle the `catalog:` protocol when deploying a package. This is no longer necessary with newer version of pnpm which handle injected workspace packages using the `catalog:` protocol out of the box.
