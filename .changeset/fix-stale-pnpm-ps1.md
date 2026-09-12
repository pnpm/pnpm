---
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

On Windows, upgrading pnpm no longer leaves a stale `pnpm.ps1` behind. PowerShell resolves `pnpm.ps1` ahead of `pnpm.cmd`, so a shim written by an older installation kept running the previous version. Linking the pnpm CLI's bins now deletes it [#13919](https://github.com/pnpm/pnpm/issues/13919).
