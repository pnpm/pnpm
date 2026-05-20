---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Fix the `minimumReleaseAge` (publishedBy) maturity shortcut to be inclusive at the cutoff. Previously, abbreviated metadata whose `modified` field equalled the cutoff fell off the fast path and triggered a full-metadata re-fetch (or a `MISSING_TIME` error when full metadata wasn't permitted). Since `modified` is an upper bound on every version's publish time, `modified == publishedBy` already implies every version passes the per-version `<=` filter in `filterPkgMetadataByPublishDate`, so the shortcut now accepts the boundary case directly. Strictly `>` (was `>=`) at the rejection branch.
