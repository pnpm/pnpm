---
"pnpm": patch
---

Redirect log output to stderr when running `pnpm sbom` so that warnings (e.g., engine mismatch) don't pollute stdout and break JSON output [#10923](https://github.com/pnpm/pnpm/issues/10923).
