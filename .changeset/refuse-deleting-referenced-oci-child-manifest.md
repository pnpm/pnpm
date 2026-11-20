---
"@pnpm/pnpr": patch
---

`DELETE /v2/<name>/manifests/<digest>` now answers `403 DENIED` when a retained image index still references the manifest [zkochan/pnpm-tasks#48](https://github.com/zkochan/pnpm-tasks/issues/48).
