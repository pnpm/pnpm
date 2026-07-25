---
"pacquet": patch
---

`pnpm run <script> -- <args>` now forwards the `--` separator to the script, matching the pnpm CLI. Previously it was dropped, so the program the script invokes read the following tokens as its own options — `pnpm run build -- --flag` against a `node`-based script failed with `node: bad option: --flag` instead of passing `--flag` to the script. `pnpm stop` and `pnpm restart` were affected the same way.
