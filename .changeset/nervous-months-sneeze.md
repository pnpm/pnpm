---
"pnpm": patch
---

The `.pnpm-debug.log` file is not written when pnpm CLI exits with an expected non-zero exit code. For instance, when vulnerabilities are found by the `pnpm audit` command.
