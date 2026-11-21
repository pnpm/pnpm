---
"pacquet": minor
---

A failed `devEngines.runtime` or `engines.runtime` check now reports the reason in its error code. A runtime that is not installed fails with `ERR_PNPM_RUNTIME_NOT_FOUND`. A runtime entry without a version range fails with `ERR_PNPM_MISSING_RUNTIME_VERSION_RANGE`, and one with an invalid range fails with `ERR_PNPM_INVALID_RUNTIME_VERSION_RANGE`. An installed version that does not satisfy the range still fails with `ERR_PNPM_BAD_RUNTIME_VERSION`.
