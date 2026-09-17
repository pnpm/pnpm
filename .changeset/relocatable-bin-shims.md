---
"pacquet": minor
---

Command shims in `node_modules/.bin` now find their `NODE_PATH` relative to their own location on macOS and Linux, so a project's bins keep working after the project directory is moved or copied [#6937](https://github.com/pnpm/pnpm/issues/6937).
