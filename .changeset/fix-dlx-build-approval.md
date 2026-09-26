---
"pacquet": patch
"@pnpm/exec.commands": patch
"pnpm": patch
---

`pnpm dlx` and `pnx` now prompt to approve dependency build scripts in interactive terminals. Cached packages with pending builds also prompt for approval. Without an interactive terminal, use `--allow-build` to allow the required builds. Fixes [pnpm/pnpm#14943](https://github.com/pnpm/pnpm/issues/14943).
