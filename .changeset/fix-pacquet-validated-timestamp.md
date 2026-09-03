---
"pacquet": patch
---

`pnpm run` and `pnpm exec` now start without reinstalling on filesystems that keep sub-millisecond mtimes, such as NTFS. Previously, every run on those filesystems reinstalled first [pnpm/pnpm#14486](https://github.com/pnpm/pnpm/issues/14486).
