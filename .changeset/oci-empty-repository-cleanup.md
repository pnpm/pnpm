---
"@pnpm/pnpr": patch
---

Deleting the last manifest of an OCI repository now removes the repository from `/v2/_catalog`, and `GET /v2/<name>/tags/list` answers `404 NAME_UNKNOWN` until another manifest is pushed. `pnpr gc` also removes empty repository records left by earlier versions. `DELETE /v2/<name>/blobs/<digest>` now answers `403 DENIED` when a retained manifest still references the blob [zkochan/pnpm-tasks#26](https://github.com/zkochan/pnpm-tasks/issues/26).
