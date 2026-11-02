---
"pacquet": patch
---

`pnpm deploy` now keeps a symlink in the project whose target is missing from the source, such as a link into a `node_modules` directory that was removed after the build. Deploy used to drop such a link silently [#17571](https://github.com/pnpm/pnpm/issues/17571).
