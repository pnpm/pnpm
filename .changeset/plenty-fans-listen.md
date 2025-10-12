---
"@pnpm/plugin-commands-config": patch
pnpm: patch
---

Fixed `pnpm config set --location=project` incorrectly handling keys with slashes (auth tokens, registry settings) [#9884](https://github.com/pnpm/pnpm/issues/9884).
