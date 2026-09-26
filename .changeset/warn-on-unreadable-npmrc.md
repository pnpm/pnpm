---
"pacquet": patch
---

pnpm now prints a warning when a `.npmrc`, `auth.ini`, or the file set by `npmrcAuthFile` exists but cannot be read. The settings in such a file were ignored without any message. A `.npmrc` that contains invalid UTF-8 is now read [#5065](https://github.com/pnpm/pnpm/issues/5065).
