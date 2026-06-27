---
"@pnpm/exe": patch
"pnpm": patch
---

node-gyp's `gyp_main.py` and `gyp` entrypoints are now packed with the executable bit in the `pnpm` and `@pnpm/exe` tarballs. Without it, building native addons from source could fail with a permission error.
