---
"@pnpm/config.normalize-registries": patch
"@pnpm/config.reader": patch
"@pnpm/types": patch
"pnpm": patch
"pacquet": patch
"@pnpm/pnpr": patch
---

Delegated installs now preserve every prefix declared for the same registry. Dependencies and existing lockfile entries using another declared prefix resolve with the smallest prefix as their package key.
