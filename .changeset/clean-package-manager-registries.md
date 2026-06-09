---
"@pnpm/config": patch
"pnpm": patch
---

Package-manager bootstrap traffic is now resolved through trusted registries and trusted network config. When pnpm downloads the pnpm version requested by a repository's `packageManager` field, the registry it fetches from (and the proxy/TLS settings used for that traffic) now come exclusively from trusted config sources — CLI options, env config, user and global `.npmrc` — defaulting to the public npm registry, instead of the repository's project/workspace settings.
