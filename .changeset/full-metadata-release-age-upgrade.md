---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

With `minimumReleaseAge` set and full metadata in use, pnpm now keeps using cached metadata that the registry confirms is current, also when that metadata has no publish times. Previously, pnpm downloaded such metadata again on every install.
