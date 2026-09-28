---
"@pnpm/pnpr": patch
---

pnpm clients can now install packages from upstream registries whose packuments omit `dist.integrity`. pnpr computes and caches each tarball's integrity before returning the normalized packument [pnpm/tasks#24](https://github.com/pnpm/tasks/issues/24).
