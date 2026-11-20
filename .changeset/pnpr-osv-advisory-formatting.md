---
"@pnpm/pnpr": patch
---

`POST /-/npm/v1/security/advisories/bulk` now preserves explicit OSV versions alongside semver ranges, skips Git ranges, and applies `limit` bounds. It also derives severities from CVSS vectors and links non-GHSA advisories to valid URLs.
