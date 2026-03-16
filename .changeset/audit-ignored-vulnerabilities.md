---
"@pnpm/plugin-commands-audit": patch
"pnpm": patch
---

Fixed an issue where `pnpm audit` incorrectly reported the number of ignored vulnerabilities when a vulnerable dependency with multiple occurrences was ignored.
