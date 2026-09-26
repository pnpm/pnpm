---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.registry.types": patch
"pnpm": patch
"pacquet": patch
---

pnpm no longer revalidates cached registry metadata when the registry sends `Cache-Control: max-age=0`, `no-cache`, or `no-store`. It downloads the metadata again, so a version newly published to such a registry is visible on the next install [#13487](https://github.com/pnpm/pnpm/issues/13487).
