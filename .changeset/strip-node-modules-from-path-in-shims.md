---
"pacquet": patch
"pnpm": patch
---

Stripped `node_modules` and relative entries from `PATH` in POSIX shim headers while resolving shell helpers, preventing decoys on Nix where `command -p` falls back to `PATH` [pnpm/pnpm#14883](https://github.com/pnpm/pnpm/issues/14883).
