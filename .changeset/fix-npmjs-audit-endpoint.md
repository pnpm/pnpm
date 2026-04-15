---
"@pnpm/deps.compliance.audit": major
"@pnpm/deps.compliance.commands": major
"@pnpm/types": major
"pnpm": major
---

`pnpm audit` now calls npm's `/-/npm/v1/security/advisories/bulk` endpoint. The legacy `/-/npm/v1/security/audits{,/quick}` endpoints have been retired by the registry, so the legacy request/response contract is no longer supported.

The new endpoint returns only `id`, `url`, `title`, `severity`, `vulnerable_versions`, and `cwe` per advisory. Everything else is computed locally:

- `findings[].paths` are computed by walking the lockfile and matching `vulnerable_versions` via semver.
- `metadata.vulnerabilities` counts advisories per severity.
- `metadata.dependencies`, `devDependencies`, `optionalDependencies`, and `totalDependencies` are computed from the lockfile.
- `patched_versions` is inferred from `vulnerable_versions` for the common `<X.Y.Z` / `<=X.Y.Z` patterns so `pnpm audit --fix` still produces usable overrides. When inference fails, it is left undefined and `pnpm audit --ignore-unfixable` treats those advisories as having no known fix.
- `github_advisory_id` is parsed from each advisory's `url`.
- `info` severity advisories are now supported across `--audit-level`, filters, and output.

### Shape changes to `AuditReport` / `AuditAdvisory`

Fields the bulk endpoint doesn't return have been removed from both types (major bump). `AuditReport` now contains only `advisories` and `metadata`. `AuditAdvisory` contains only `findings`, `id`, `title`, `module_name`, `vulnerable_versions`, `patched_versions`, `severity`, `cwe`, `github_advisory_id`, and `url`. Consumers that relied on `actions`, `muted`, `cves`, `created`, `updated`, `deleted`, `access`, `overview`, `recommendation`, `references`, `found_by`, `reported_by`, or `metadata` on advisories need to update.

The bulk endpoint does not return CVE identifiers. CVE-based filtering has been replaced with GitHub advisory ID (GHSA) filtering:

- `auditConfig.ignoreCves` → `auditConfig.ignoreGhsas` (the previous key is no longer recognized)
- `pnpm audit --ignore <id>` / `pnpm audit --ignore-unfixable` now read and write GHSAs instead of CVEs
- GHSAs are derived from each advisory's `url` (`https://github.com/advisories/GHSA-xxxx-xxxx-xxxx`)

To migrate: replace each `CVE-YYYY-NNNNN` entry in your `auditConfig.ignoreCves` with the corresponding `GHSA-xxxx-xxxx-xxxx` value (visible in the `More info` column of `pnpm audit` output) and move it under `auditConfig.ignoreGhsas`.
