---
"@pnpm/plugin-commands-audit": patch
"@pnpm/config": patch
"pnpm": patch
---

Fixed `pnpm audit --json` to respect the `--audit-level` setting for both exit code and output filtering.
