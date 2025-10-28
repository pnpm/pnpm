---
"@pnpm/registry.pkg-metadata-filter": patch
"pnpm": patch
---

When the `latest` version doesn't satisfy the maturity requirement configured by `minimumReleaseAge`, pick the highest version that is mature enough, even if it has a different major version [#10100](https://github.com/pnpm/pnpm/issues/10100).

