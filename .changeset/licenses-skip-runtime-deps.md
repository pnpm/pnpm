---
"@pnpm/deps.compliance.license-scanner": patch
"@pnpm/store.pkg-finder": patch
"pnpm": patch
---

`pnpm licenses list` now reports the runtime downloaded through `devEngines.runtime` with `onFail: "download"`. The command previously failed with `ERR_PNPM_UNSUPPORTED_PACKAGE_TYPE` [#14172](https://github.com/pnpm/pnpm/issues/14172).
