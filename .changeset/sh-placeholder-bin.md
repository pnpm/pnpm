---
"pacquet": patch
---

pnpm now runs through Node.js when it was installed by a tool that skips build scripts, such as Vercel's `packageManager` provisioning, Bun, Deno, or `npm install --ignore-scripts`. Those installs previously failed with `syntax error near unexpected token ')'`. On Windows they still cannot run pnpm [#14346](https://github.com/pnpm/pnpm/issues/14346).
