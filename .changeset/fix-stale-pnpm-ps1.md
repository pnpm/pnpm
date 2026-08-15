---
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

Fix `pnpm self-update` leaving behind a stale `pnpm.ps1` shim on Windows. Any existing `.ps1` shim for `pnpm` (or other commands that disable the PowerShell shim) is now deleted during bin linking to prevent it from shadowing the updated binary [pnpm/pnpm#13919](https://github.com/pnpm/pnpm/issues/13919).
