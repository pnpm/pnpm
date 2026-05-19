---
"@pnpm/global.commands": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

Fix global add/update to handle minimumReleaseAge policy violations instead of surfacing an internal resolver guardrail error.
