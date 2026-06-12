---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

A `304 Not Modified` answer from the registry now renews the cached metadata file's mtime, so the `minimumReleaseAge` freshness shortcut keeps serving resolutions from the cache. Previously, once a cached packument grew older than `minimumReleaseAge`, every subsequent install re-validated it against the registry forever, because a 304 never rewrites the file.
