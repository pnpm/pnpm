---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

Stop repeating the "Lockfile passes supply-chain policies (verified Nh ago)" line during installs. The cached verdict is emitted once and then cleared from the rolling region of the reporter, so subsequent progress redraws no longer re-include it. Previously the line stayed in `acc.blocks` for the rest of the command and was re-rendered on every progress tick — once per redraw — producing dozens of duplicate lines in captured output (CI logs, `tee`, `script`, terminal scrollback).
