---
"@pnpm/plugin-commands-installation": patch
"@pnpm/plugin-commands-rebuild": patch
"pnpm": patch
---

Replace environment variable placeholders with their values, when reading `.npmrc` files in subdirectories inside a workspace [#2570](https://github.com/pnpm/pnpm/issues/2570).
