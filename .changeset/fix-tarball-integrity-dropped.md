---
"@pnpm/installing.package-requester": patch
"pnpm": patch
---

Fix the `integrity` field being dropped from the lockfile entry of a remote (non-registry) https-tarball dependency when an unrelated package is installed afterwards. URL/tarball resolvers do not return an integrity (it is only known after the tarball is downloaded), so when such a dependency was reused from the lockfile without being re-fetched, its integrity was lost. It is now carried over from the existing resolution. With pnpm's lockfile-integrity hardening, the missing integrity made subsequent `--frozen-lockfile` installs fail with `ERR_PNPM_MISSING_TARBALL_INTEGRITY`. [#12001](https://github.com/pnpm/pnpm/issues/12001).
