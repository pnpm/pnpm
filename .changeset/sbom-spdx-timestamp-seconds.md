---
"@pnpm/deps.compliance.sbom": patch
"@pnpm/deps.compliance.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm sbom --sbom-format spdx` now writes `creationInfo.created` truncated to whole seconds (e.g. `2026-09-08T10:38:21Z`) instead of including fractional seconds, which SPDX 2.3 §6.9 forbids. Strict SPDX consumers rejected the previous output [#14684](https://github.com/pnpm/pnpm/issues/14684).
