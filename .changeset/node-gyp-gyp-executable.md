---
"@pnpm/exe": patch
"pnpm": patch
---

Restore the execute bit on the `node-gyp` gyp entrypoints packed inside `pnpm` and `@pnpm/exe` (`dist/node_modules/node-gyp/gyp/gyp_main.py` and `dist/node_modules/node-gyp/gyp/gyp`). These ship with shebangs and are exec'd directly by gyp's `make` generator when it regenerates the `Makefile` during a from-source native addon build. Without the execute bit they are packed at 0644, so the build fails with `Permission denied` / `make: *** [Makefile] Error 126`. This is a follow-up to [#11485](https://github.com/pnpm/pnpm/pull/11485), which restored the bit on the `node-gyp` bin shims but missed the two gyp entrypoints [#12455](https://github.com/pnpm/pnpm/issues/12455).
