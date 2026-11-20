---
"@pnpm/pnpr": minor
---

pnpr stores packages under readable paths. `@scope/pkg` is stored in `packages/@scope/pkg/`, an unscoped package in `packages/@/<name>/`, and a tarball as `blobs/<version>.tgz`.

A store written by an earlier pnpr is not read. Publish its packages again into an empty store.
