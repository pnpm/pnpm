---
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
"pacquet": patch
---

pnpm no longer hangs after a lifecycle script exits while a process it started in the background keeps the script's output open. pnpm stops reading that output one second after the script exits [#5730](https://github.com/pnpm/pnpm/issues/5730).
