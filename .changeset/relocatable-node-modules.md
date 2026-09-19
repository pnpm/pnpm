---
"pacquet": minor
---

`pnpm install`, `pnpm run`, and `pnpm exec` now reuse a `node_modules` directory that moved or was copied together with its project. The first command after the move checks the tree and records its new location. Reuse works on macOS and Linux, for a project moved with `mv`, `cp -a`, or a copy-on-write clone [#6937](https://github.com/pnpm/pnpm/issues/6937).
