---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

Fix a bug in which `pnpm add` downloads packages whose `libc` differ from `pnpm.supportedArchitectures.libc`.
