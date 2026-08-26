---
"@pnpm/deps.compliance.license-scanner": patch
"pnpm": patch
"pacquet": patch
---

`pnpm licenses list` no longer fails with `ERR_PNPM_UNSUPPORTED_PACKAGE_TYPE` in a project that downloads its runtime through `devEngines.runtime` with `onFail: "download"`. The downloaded runtime is skipped instead of being treated as a licensed dependency [#14172](https://github.com/pnpm/pnpm/issues/14172).
