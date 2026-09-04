---
"pacquet": patch
---

pnpm now runs through Node.js when it was installed by a tool that skips build scripts, such as Vercel's `packageManager` provisioning, Bun, Deno, or `npm install --ignore-scripts`. Those installs previously failed with `syntax error near unexpected token ')'`. They still cannot run pnpm on Windows. On macOS only a shell can start it [#14346](https://github.com/pnpm/pnpm/issues/14346).
