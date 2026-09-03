---
"@pnpm/engine.pm.commands": patch
"pacquet": patch
"pnpm": patch
---

pnpm 11 now runs projects pinned to pnpm 12.3 or later without passing the native pnpm binary to Node.js. Future pnpm 12 npm wrappers keep their placeholder shebang-less so older pnpm 11 releases can install them through the version store. Wrapper installs must allow lifecycle scripts to install the native binary [#14502](https://github.com/pnpm/pnpm/issues/14502).
