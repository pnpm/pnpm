---
"@pnpm/deps.compliance.audit": major
"@pnpm/deps.compliance.commands": patch
"pnpm": minor
---

`pnpm audit` now calls npm's `/-/npm/v1/security/advisories/bulk` endpoint. The legacy `/-/npm/v1/security/audits{,/quick}` endpoints have been retired by the registry, so the legacy request/response contract is no longer supported. The audit tree is flattened to the bulk request shape, and the per-package advisory arrays returned by the new endpoint are mapped back to the `AuditReport` shape consumed by downstream commands. Advisory `findings[].paths` and `metadata.vulnerabilities` counts are now computed locally from the lockfile, since the new endpoint does not return them. The `actions` field on the audit report is no longer populated — remediation is still performed by `pnpm audit --fix`, which derives fixes from advisories.
