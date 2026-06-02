---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Skip lockfile `minimumReleaseAge`/`trustPolicy` verification for non-registry tarball protocols (for example `file:`), so local tarball dependencies are not incorrectly checked against npm registry metadata.
