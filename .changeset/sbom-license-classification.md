---
"pacquet": patch
---

`pnpm sbom` now emits a license value that is not an SPDX identifier, such as `UNLICENSED` or a misspelled identifier, as a CycloneDX license name. Such a value used to be emitted as a `license.id`, which made the whole CycloneDX document fail schema validation [pnpm/pnpm#14786](https://github.com/pnpm/pnpm/issues/14786).
