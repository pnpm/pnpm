---
"pacquet": patch
---

pnpm now honors the first setting of an `.npmrc` file that starts with a UTF-8 byte order mark. The mark was previously read as part of the first key, so the setting was silently dropped and the default applied instead [pnpm/pnpm#15353](https://github.com/pnpm/pnpm/issues/15353).
