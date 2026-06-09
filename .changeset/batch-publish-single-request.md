---
"@pnpm/releasing.commands": minor
"pnpm": minor
---

Added a new opt-in `--batch` flag to `pnpm publish --recursive` that sends all selected packages to the registry in a single `PUT /-/pnpm/v1/publish` request instead of one request per package. The target registry has to implement the batch publish endpoint (pnpr does); registries that don't are reported with a clear `ERR_PNPM_BATCH_PUBLISH_UNSUPPORTED` error. The batch is processed all-or-nothing by pnpr: if any package in the batch fails validation, none of the packages are published.
