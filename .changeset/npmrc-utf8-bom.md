---
"pacquet": patch
---

pnpm now parses the first setting in a `.npmrc` that starts with a UTF-8 byte order mark. Previously, the leading byte order mark caused the first line's key to be ignored [#15353](https://github.com/pnpm/pnpm/issues/15353).
