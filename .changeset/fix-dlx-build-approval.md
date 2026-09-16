---
"pacquet": patch
---

`pnpm dlx` and `pnx` now prompt to approve dependency build scripts in interactive terminals. In noninteractive environments, use `--allow-build` to allow the required builds. Fixes [pnpm/pnpm#14943](https://github.com/pnpm/pnpm/issues/14943).
