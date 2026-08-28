---
"@pnpm/cli.default-reporter": patch
"pacquet": patch
"pnpm": patch
---

The progress output no longer overwrites the lines above it once it grows taller than the terminal window [#14270](https://github.com/pnpm/pnpm/issues/14270).
