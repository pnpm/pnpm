---
"@pnpm/deps.compliance.sbom": patch
"pacquet": patch
"pnpm": patch
---

`pnpm sbom --sbom-format spdx` now writes `creationInfo.created` with whole seconds, such as `2026-09-08T10:38:21Z`. The timestamp carried fractional seconds, which strict SPDX consumers rejected [#14684](https://github.com/pnpm/pnpm/issues/14684).
