---
"pacquet": minor
---

`pnpm install`, `pnpm run`, and `pnpm exec` on macOS and Linux can now reuse a compatible `node_modules` directory that was moved or copied together with its project, for example with `mv`, `cp -a`, or a copy-on-write clone. The first command after the move checks the tree and records its new location [#6937](https://github.com/pnpm/pnpm/issues/6937).
