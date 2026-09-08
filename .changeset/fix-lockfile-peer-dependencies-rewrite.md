---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed manifest edits leaking between dependencies that resolve to the same package version. A `readPackage` hook that edits its argument in place, or a deprecation notice carried over from the lockfile, was written into the manifest the resolver caches and then reused for the next dependent [#13988](https://github.com/pnpm/pnpm/issues/13988).
