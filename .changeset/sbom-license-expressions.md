---
"@pnpm/deps.compliance.sbom": patch
"pnpm": patch
"pacquet": patch
---

`pnpm sbom` now emits a license value as a CycloneDX expression only when it is a valid SPDX license expression. Anything else is emitted as a CycloneDX license name [pnpm/pnpm#14786](https://github.com/pnpm/pnpm/issues/14786).
