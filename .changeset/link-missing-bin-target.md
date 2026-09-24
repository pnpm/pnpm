---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"pnpm": patch
---

`pnpm install` now links a dependency's bin even when the bin's file does not exist yet, such as a workspace package's bin that a build script creates after install. Previously pnpm printed a `Failed to create bin` warning. The command then stayed missing until `node_modules` was removed [pnpm/pnpm#10007](https://github.com/pnpm/pnpm/issues/10007), [pnpm/pnpm#10216](https://github.com/pnpm/pnpm/issues/10216).
