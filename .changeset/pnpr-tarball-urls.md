---
"@pnpm/pnpr": patch
---

Served `dist.tarball` URLs on `.tgz` registries now use the `<name>@<version>` route when a version's tarball filename matches `<scopeless-name>-<version>.tgz`. Upstream versions with non-canonical tarball filenames keep the `/-/<filename>` route.
