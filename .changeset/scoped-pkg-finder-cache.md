---
"@pnpm/store.pkg-finder": patch
"@pnpm/deps.compliance.license-scanner": patch
---

`readPackageFileMap` accepts an optional `indexCache` map. Callers that walk a
dependency graph can share one decoded `PackageFilesIndex` across visits to
peer-dep variants that resolve to the same store entry, avoiding repeated
SQLite reads and msgpack decodes. The license scanner now passes one such
cache for the lifetime of a single `lockfileToLicenseNodeTree` call.
