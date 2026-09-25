---
"pacquet": patch
---

A pnpmfile `fetchers` hook no longer runs twice when a `resolvers` hook returns a tarball resolution without a manifest. The archive fetched during resolution is now reused during installation [pnpm/pnpm#15025](https://github.com/pnpm/pnpm/issues/15025).
