---
"pnpm": patch
---

Binaries of runtime engines (Node.js, Deno, Bun) are written to `node_modules/.bin` before lifecycle scripts (install, postinstall, prepare) are executed [#10244](https://github.com/pnpm/pnpm/issues/10244).
