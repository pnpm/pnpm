---
"@pnpm/pnpr": minor
---

`pnpr gc` removes the archives that no hosted package maps, in every hosted npm, PyPI, Cargo, and OCI registry. It runs while the registry keeps serving and other replicas keep publishing. It removes the bytes of unpublished versions and packages, image blobs that no retained manifest reaches, and the archives of publishes that stopped or were refused at their commit. It also finishes interrupted reclaims.

The new `gc.lease` setting sets how long an interrupted reclaim is left before another run finishes it. It defaults to `1h`.

`pnpr verify` now only reports orphaned blobs. Its `--prune-orphans` and `--min-age` options are removed.

A hosted registry stores identical bytes once per org and ecosystem, whichever packages, versions, or image repositories publish them. A publish whose bytes are stored already stores nothing. A tarball `DELETE` of a version that the package still lists unpublishes that version.

Existing hosted stores are not migrated. Point `storage` or the `s3:` prefix at a fresh directory or prefix, wipe pnpr's metadata tables, and republish. The SQL metadata schema version is `0.2` [pnpm/tasks#269](https://github.com/pnpm/tasks/issues/269).
