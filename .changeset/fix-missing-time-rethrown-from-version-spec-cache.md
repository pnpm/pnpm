---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Fix `ERR_PNPM_MISSING_TIME` being thrown from the `spec.type === 'version'` cache path in `pickPackage` when `minimumReleaseAge` is configured and `minimumReleaseAgeStrict` is true (the default when the user explicitly sets `minimumReleaseAge` as of v11.1.0). The catch block now filters out `ERR_PNPM_MISSING_TIME` errors and falls through to the registry-fetch path, matching the behavior of the immediately-following mtime-gated cache block.
