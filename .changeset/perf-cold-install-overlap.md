---
"pacquet": patch
---

Faster installs when the caches are cold: the lockfile-verification pass now runs concurrently with resolution and materialization instead of in front of them (its verdict still gates bin linking, dependency builds, and the lockfile write), its registry requests no longer delay the resolver's own metadata fetches, newly downloaded packages are linked into the virtual store while the remaining downloads are still in flight, and the verification verdict is mirrored next to the current lockfile in the virtual store so wiping the cache directory alone no longer forces a full re-verification of an unchanged, already-installed lockfile.
