---
"pnpm": patch
---

Fixed a Windows-only hang where a failed command could take 20–46 seconds to exit. On error, pnpm enumerates descendant processes (via `pidtree`) to terminate them, which on Windows shells out to `wmic`/PowerShell `Get-CimInstance Win32_Process` — a lookup that is extremely slow on some machines. The lookup is now bounded by a short timeout so it can no longer stall the process exit.
