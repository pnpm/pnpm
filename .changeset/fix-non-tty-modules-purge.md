---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm install` now automatically proceeds with removing and reinstalling the default `node_modules` directory in non-interactive environments such as Docker containers, CI pipelines, and piped commands [#6778](https://github.com/pnpm/pnpm/issues/6778), [#9166](https://github.com/pnpm/pnpm/issues/9166), [#8654](https://github.com/pnpm/pnpm/issues/8654), [#8085](https://github.com/pnpm/pnpm/issues/8085).
