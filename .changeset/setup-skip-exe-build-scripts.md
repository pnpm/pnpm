---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

`pnpm setup` no longer prompts to approve build scripts for `@pnpm/exe` when installing the standalone executable. pnpm links the platform-specific binary itself, so the package's install scripts are skipped during the global self-install [#12377](https://github.com/pnpm/pnpm/issues/12377).
