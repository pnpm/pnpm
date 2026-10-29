---
"pacquet": patch
---

Honor `--dir` and `--workspace-root` when running commands through `dlx`, `pnpx`, and `create`. Without either flag, commands still run in the invocation directory.
