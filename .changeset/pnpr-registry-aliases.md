---
"@pnpm/config.normalize-registries": patch
"@pnpm/types": patch
"pnpm": patch
"pacquet": patch
"@pnpm/pnpr": patch
---

Installs delegated to a pnpr server now resolve every prefix declared for a registry URL. A dependency or lockfile entry that used the second of two prefixes for the same registry, such as `zeta:` next to `work:` in `namedRegistries`, failed on the server [pnpm/tasks#204](https://github.com/pnpm/tasks/issues/204).
