---
"@pnpm/resolving.registry.pkg-metadata-filter": patch
"pacquet": patch
"pnpm": patch
---

Fixed `minimumReleaseAge` fallback for custom dist-tags so the selected version does not exceed the registry’s original tag target.
