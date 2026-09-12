---
"pacquet": patch
---

Fixed `pnpm install` and `pnpm dlx` failing with "Permission denied" on Android when the filesystem denies hardlinks or reflinks. Automatic package imports now fall back to copying on `EACCES` [pnpm/pnpm#14780](https://github.com/pnpm/pnpm/issues/14780).
