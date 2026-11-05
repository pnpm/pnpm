---
"@pnpm/pnpr": minor
---

`pnpr gc` removes the archives no hosted package maps, in every hosted npm, PyPI, Cargo, and OCI registry, while the registry keeps serving and other replicas keep publishing: the bytes of unpublished versions and packages, image blobs no retained manifest reaches, what a publish that stopped before its commit or was refused at it stored, and reclaims that were interrupted. Every removal is fenced by the metadata database rather than by timing. The new `gc.lease` setting (default `1h`) sets how long an interrupted reclaim is left before another run finishes it. `pnpr verify` no longer removes orphaned blobs, and its `--prune-orphans` and `--min-age` options are gone.

A hosted registry now stores each archive by the identity of its bytes, so identical bytes are stored once per org and ecosystem, whichever packages, versions, or image repositories publish them, and the metadata maps every published file to them. A publish that finds its bytes stored already stores nothing, and a tarball `DELETE` of a version still listed unpublishes that version. Existing hosted stores are not migrated: point `storage` (or the `s3:` prefix) at a fresh directory or prefix, wipe pnpr's metadata tables (the SQL metadata schema version is `0.2`), and republish.
