---
"pacquet": patch
---

pnpm now prints a warning when a `.npmrc`, `auth.ini`, or the file set by `npmrcAuthFile` exists but cannot be read. The settings in such a file were ignored without any message [#5065](https://github.com/pnpm/pnpm/issues/5065). A `.npmrc` that contains invalid UTF-8 is now read.
