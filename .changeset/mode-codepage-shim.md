---
'@pnpm/bins.cmd-shim': patch
'pnpm': patch
'pacquet': patch
---

`pnpm run` no longer clears the visible terminal on Windows when a project path contains non-ASCII characters. Command shims now switch the console code page with `mode.com con cp select=` instead of `chcp.com`; changing the code page with `chcp.com` clears the screen.
