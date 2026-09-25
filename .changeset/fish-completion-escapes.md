---
"@pnpm/cli.commands": patch
"pnpm": patch
---

Fish shell completion now omits names containing backslashes. The completion template could interpret them as escape sequences and inject extra completion records or terminal controls.
