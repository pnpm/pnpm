---
"pacquet": patch
---

Fixed a pnpmfile `fetchers` hook running twice when a `resolvers` hook returned a tarball resolution without supplying a manifest. The archive fetched during resolution is now reused by the install pass [pnpm/pnpm#15025](https://github.com/pnpm/pnpm/issues/15025).
