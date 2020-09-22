---
"@pnpm/global-bin-dir": patch
---

When searching for a suitable global bin directory, search for symlinked node, npm, pnpm commands, not only command files.
