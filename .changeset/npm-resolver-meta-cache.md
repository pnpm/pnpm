---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

The in-memory package metadata cache is now populated on the exact-version disk fast path, so repeated resolutions of the same package within one install no longer re-read and re-parse the on-disk metadata. In large monorepos this brings the time for adding a new package down from minutes to seconds. The in-memory cache key now also includes the registry, so a package of the same name served by two different registries in a single install can no longer share a cache slot and resolve the wrong tarball.
