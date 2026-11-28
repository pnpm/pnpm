---
"pnpm": patch
---

`pnpm deploy` with `--legacy` or `forceLegacyDeploy` now deploys a project whose manifest is `package.yaml` or `package.json5`. It failed with `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND`.
