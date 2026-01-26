---
"@pnpm/npm-resolver": patch
"@pnpm/fetching-types": patch
"@pnpm/package-requester": patch
"pnpm": patch
---

Fixed optional dependencies to request full metadata from the registry to get the `libc` field, which is required for proper platform compatibility checks [#9950](https://github.com/pnpm/pnpm/issues/9950).
