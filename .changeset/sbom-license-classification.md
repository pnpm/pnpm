---
"@pnpm/deps.compliance.sbom": patch
"pnpm": patch
"pacquet": patch
---

`pnpm sbom` now emits non-SPDX license strings as CycloneDX license names and validates SPDX expressions before emitting them [pnpm/pnpm#14786](https://github.com/pnpm/pnpm/issues/14786).
