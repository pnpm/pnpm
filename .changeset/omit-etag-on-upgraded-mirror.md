---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Fixed `minimumReleaseAge` making pnpm download a package's full metadata again on every install. The cached copy carried a validator the registry could not match, so pnpm could never revalidate it [pnpm/pnpm#15103](https://github.com/pnpm/pnpm/issues/15103).
