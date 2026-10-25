---
"@pnpm/pnpr": patch
---

Served `dist.tarball` URLs on `.tgz` registries now use the `<name>@<version>` route. The `/-/<filename>` route remains supported for existing lockfiles and older clients.
