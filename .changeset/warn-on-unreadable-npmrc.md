---
"pacquet": patch
---

pnpm now prints a warning when a `.npmrc` file exists but cannot be read, such as when its permissions deny access. The settings in that file were ignored without any message [#5065](https://github.com/pnpm/pnpm/issues/5065). A `.npmrc` with bytes that are not valid UTF-8 is now read instead of being ignored.
