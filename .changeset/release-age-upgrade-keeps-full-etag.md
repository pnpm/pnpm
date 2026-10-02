---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.registry.types": patch
"pacquet": patch
"pnpm": patch
---

Cached metadata for a package published within `minimumReleaseAge` is now revalidated with its ETag, so the npm registry can answer `304 Not Modified`. Before, the next install that checked the cache downloaded the whole document again.
