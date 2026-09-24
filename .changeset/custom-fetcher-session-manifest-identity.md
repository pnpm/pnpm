---
"pacquet": patch
---

Fixed a pnpmfile `fetchers` hook running twice for the same package when a `resolvers` hook resolved it to a tarball without a manifest. The archive fetched during resolution is now reused by the install pass [pnpm/pnpm#15025](https://github.com/pnpm/pnpm/issues/15025).
