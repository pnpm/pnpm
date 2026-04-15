---
"@pnpm/deps.compliance.audit": major
"@pnpm/deps.compliance.commands": major
"@pnpm/types": major
"pnpm": major
---

`pnpm audit` now calls npm's `/-/npm/v1/security/advisories/bulk` endpoint. The legacy `/-/npm/v1/security/audits{,/quick}` endpoints have been retired by the registry, so the legacy request/response contract is no longer supported. The audit tree is flattened to the bulk request shape, and the per-package advisory arrays returned by the new endpoint are mapped back to the `AuditReport` shape consumed by downstream commands. Advisory `findings[].paths` and `metadata.vulnerabilities` counts are now computed locally from the lockfile, since the new endpoint does not return them. `patched_versions` is inferred from `vulnerable_versions` when the range has the common `<X.Y.Z` or `<=X.Y.Z` shape. The `actions` field on the audit report is no longer populated — `pnpm audit --fix` still works, it derives fixes directly from the advisories.

The bulk endpoint does not return CVE identifiers at all. As a consequence, CVE-based filtering has been replaced with GitHub advisory ID (GHSA) filtering:

- `auditConfig.ignoreCves` → `auditConfig.ignoreGhsas` (the previous key is no longer recognized)
- `pnpm audit --ignore <id>` / `pnpm audit --ignore-unfixable` now read and write GHSAs instead of CVEs
- GHSAs are derived from each advisory's `url` (`https://github.com/advisories/GHSA-xxxx-xxxx-xxxx`)

To migrate: replace each `CVE-YYYY-NNNNN` entry in your `auditConfig.ignoreCves` with the corresponding `GHSA-xxxx-xxxx-xxxx` value (visible in the `More info` column of `pnpm audit` output) and move it under `auditConfig.ignoreGhsas`.
