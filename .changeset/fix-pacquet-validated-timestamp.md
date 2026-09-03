---
"pacquet": patch
---

`pnpm run` and `pnpm exec` no longer reinstall before every run on filesystems that keep sub-millisecond mtimes, such as NTFS. The freshness baseline that `pnpm install` records now covers the manifest it validated [pnpm/pnpm#14486](https://github.com/pnpm/pnpm/issues/14486).
