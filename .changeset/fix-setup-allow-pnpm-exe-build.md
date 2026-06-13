---
"@pnpm/pm.commands": patch
"pnpm": patch
---

Fixed `pnpm setup` halting on 11.6.0's per-package build-script consent prompt for `@pnpm/exe` itself, which looked like a supply-chain warning for pnpm's own install path. The internal `pnpm add -g file:<dir>` invocation now passes `--allow-build=@pnpm/exe` so the prompt is skipped for pnpm's own binary placement. [pnpm/pnpm#12377](https://github.com/pnpm/pnpm/issues/12377)
