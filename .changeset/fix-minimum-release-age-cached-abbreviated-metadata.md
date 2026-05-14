---
"@pnpm/npm-resolver": patch
"pnpm": patch
---

Fixed `minimumReleaseAge` handling when cached metadata is abbreviated. The npm registry returns abbreviated package metadata (without the per-version `time` field) by default, which made the maturity check throw `ERR_PNPM_MISSING_TIME` whenever cached abbreviated metadata was reused. pnpm now upgrades cached abbreviated metadata to the full document via a follow-up fetch when `minimumReleaseAge` is active, persists the upgrade to the on-disk cache so subsequent installs skip the extra fetch, and lets `ERR_PNPM_MISSING_TIME` from the cache fast-path fall through to the network fetch even under strict mode.
