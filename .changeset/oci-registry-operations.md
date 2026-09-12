---
"@pnpm/pnpr": minor
---

pnpr can resume OCI uploads across replicas when using S3 storage. Abandoned shared upload sessions expire at startup after 24 hours of inactivity.

`pnpr oci-gc --registry <name>` collects old, unreferenced image blobs while registry writers are stopped. Use `--dry-run` to preview the cleanup.
