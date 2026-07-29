---
"pacquet": patch
---

`pnpm test`, `pnpm start`, and `pnpm stop` now forward their arguments to the script, matching the pnpm CLI. `pnpm test --watch` and `pnpm start --port 3000` previously failed with a usage error, and `pnpm stop` claimed `--if-present` and `-s` for itself instead of passing them on. As with `pnpm run`, every token after the command name reaches the script verbatim, a `--` separator included.
