---
"@pnpm/deps.compliance.audit": major
"@pnpm/deps.compliance.commands": major
"@pnpm/types": major
"pnpm": major
---

`pnpm audit` now calls npm's `/-/npm/v1/security/advisories/bulk` endpoint. The legacy `/-/npm/v1/security/audits{,/quick}` endpoints have been retired by the registry, so the legacy request/response contract is no longer supported.

The new endpoint returns a slim advisory list keyed by package name, without `findings[].paths`, `actions`, `metadata`, `cves`, `patched_versions`, `github_advisory_id`, or `module_name`. The audit client now reconstructs what downstream commands need:

- `findings[].paths` are computed by walking the lockfile and matching `vulnerable_versions` via semver.
- `metadata.vulnerabilities` counts advisories per severity.
- `metadata.dependencies`, `devDependencies`, `optionalDependencies`, and `totalDependencies` are computed from the lockfile.
- `patched_versions` is inferred from `vulnerable_versions` for the common `<X.Y.Z` / `<=X.Y.Z` patterns so `pnpm audit --fix` still produces usable overrides.
- `github_advisory_id` is parsed from each advisory's `url`.
- `actions` is no longer populated — `pnpm audit --fix` derives fixes directly from advisories.
- `info` severity advisories are now supported across `--audit-level`, filters, and output.

The bulk endpoint does not return CVE identifiers at all. As a consequence, CVE-based filtering has been replaced with GitHub advisory ID (GHSA) filtering:

- `auditConfig.ignoreCves` → `auditConfig.ignoreGhsas` (the previous key is no longer recognized)
- `pnpm audit --ignore <id>` / `pnpm audit --ignore-unfixable` now read and write GHSAs instead of CVEs
- GHSAs are derived from each advisory's `url` (`https://github.com/advisories/GHSA-xxxx-xxxx-xxxx`)

To migrate: replace each `CVE-YYYY-NNNNN` entry in your `auditConfig.ignoreCves` with the corresponding `GHSA-xxxx-xxxx-xxxx` value (visible in the `More info` column of `pnpm audit` output) and move it under `auditConfig.ignoreGhsas`.
