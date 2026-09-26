---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

`pnpm self-update` writes the pnpm shim in the pnpm home bin directory as a resolved absolute path, with no `..` segments. A shim left in the pnpm home directory is written the same way [pnpm/pnpm#12865](https://github.com/pnpm/pnpm/issues/12865).
