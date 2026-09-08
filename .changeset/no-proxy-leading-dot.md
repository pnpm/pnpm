---
"pacquet": patch
---

Fixed `NO_PROXY` entries that start with a dot, such as `.npmjs.org`, never bypassing the proxy. Entries without the leading dot already worked correctly [#14686](https://github.com/pnpm/pnpm/issues/14686).
