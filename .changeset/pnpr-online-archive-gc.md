---
"@pnpm/pnpr": minor
---

`pnpr gc` removes archives that no hosted package references, in every hosted npm, PyPI, Cargo, and OCI registry, while the registry keeps serving and other replicas keep publishing. `pnpr gc --registry <name> <name>@<version>` removes the archives of a version whose publish was refused at its commit, so it can be published again with rebuilt bytes. It replaces `pnpr oci-gc` and `pnpr verify --prune-orphans`; `pnpr verify` now only reports orphaned blobs.

A publish refused at its commit now removes the archives it wrote on libSQL too, unless another replica committed them. The SQL metadata schema version is now `0.2`: wipe pnpr's metadata tables before upgrading.
