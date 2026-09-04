---
"pacquet": patch
---

Fixed `pnpm install` failing with `syntax error near unexpected token ')'` when pnpm was installed by a tool that skips build scripts, such as Vercel's `packageManager` provisioning, Bun, Deno, or `npm install --ignore-scripts` [#14346](https://github.com/pnpm/pnpm/issues/14346). pnpm now runs through Node.js in those installs until the native binary is in place. On Windows such an install still cannot run pnpm.
