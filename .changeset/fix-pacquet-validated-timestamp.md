---
"pacquet": patch
---

`pnpm install` now records a freshness baseline that covers the manifests it validated on filesystems that keep sub-millisecond mtimes, such as NTFS. Previously, `pnpm run` and `pnpm exec` reinstalled before every run on those filesystems [pnpm/pnpm#14486](https://github.com/pnpm/pnpm/issues/14486).
