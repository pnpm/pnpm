---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

With `minimumReleaseAge` set and full metadata in use, pnpm no longer downloads a package's metadata again after the registry confirms its cached copy is current. This applies to registries whose full metadata has no publish times.
