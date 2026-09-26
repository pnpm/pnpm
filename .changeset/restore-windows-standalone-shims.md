---
"@pnpm/exe": patch
"pnpm": patch
---

On Windows, globally installed `@pnpm/exe` commands now run in the invoking PowerShell console and return their exit status [pnpm/pnpm#6503](https://github.com/pnpm/pnpm/issues/6503).
