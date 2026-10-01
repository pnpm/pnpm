---
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
"pacquet": patch
---

Scripts run without a terminal no longer start a second `sh` each. One watchdog per pnpm command now ends every script's process group if pnpm is killed, so `pnpm -r run` across many projects starts half as many processes [#16489](https://github.com/pnpm/pnpm/issues/16489).
