---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

Fixed `pnpm add` failing with "Cannot read properties of undefined (reading 'manifest')" in a workspace on Windows when the current directory has a lowercase drive letter [#18175](https://github.com/pnpm/pnpm/issues/18175).
