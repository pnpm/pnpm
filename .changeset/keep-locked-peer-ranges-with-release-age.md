---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

With `minimumReleaseAge` set, re-resolving the lockfile no longer rewrites the `peerDependencies` recorded for a package whose version did not change. This happened when the registry metadata of a package differed from the `package.json` in its tarball [#13988](https://github.com/pnpm/pnpm/issues/13988).
