---
"@pnpm/pnpr": patch
---

Configuration parsing now rejects unset environment variable references in secret fields at startup. Added support for independent cache and OCI secret configuration under `secrets`, and derived domain-separated subkeys when a single root `secret` is provided. Added per-ecosystem validation rejecting `teamsManagedBy: api` and package `unpublish` rules on Cargo and PyPI registries.
