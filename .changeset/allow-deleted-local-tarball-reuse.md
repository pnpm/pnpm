---
"@pnpm/resolving.local-resolver": patch
"@pnpm/installing.package-requester": patch
"pnpm": patch
---

Installing or adding dependencies no longer fails when a previously installed local tarball file was deleted from disk [#8367](https://github.com/pnpm/pnpm/issues/8367).
