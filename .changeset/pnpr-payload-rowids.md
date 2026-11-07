---
"@pnpm/pnpr": patch
---

Changed pnpr's SQLite and libSQL metadata layout for packages with large manifests and READMEs. The metadata schema changed, so databases created by earlier pnpr builds are refused. See the [reset instructions](https://pnpm.io/pnpr/storage#metadata-schema-changes).
