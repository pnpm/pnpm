---
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnx` and `pnpm create` now run the command in the directory selected by `--dir` or `--workspace-root`. Without either flag, the command still runs in the directory pnpm was started in.
