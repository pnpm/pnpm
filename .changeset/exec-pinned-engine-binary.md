---
"pacquet": patch
---

Commands in a project that pins a different pnpm version start about 13 ms faster on macOS. pnpm now runs the pinned version's binary directly, without the shell script in front of it [pnpm/tasks#66](https://github.com/pnpm/tasks/issues/66).
