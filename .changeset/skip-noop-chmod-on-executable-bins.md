---
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

Executable linking now checks for all executable permission bits before skipping chmod. Targets with partial execute permissions previously skipped permission repair and remained non-executable for other users [pnpm/pnpm#3699](https://github.com/pnpm/pnpm/issues/3699).
