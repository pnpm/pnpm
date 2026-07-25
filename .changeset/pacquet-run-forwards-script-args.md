---
"pacquet": patch
---

`pnpm run <script> <args>` now forwards every argument after the script name to the script verbatim, matching the behavior of the JavaScript implementation. Previously the `--` separator was dropped, so `pnpm run test -- --watch` reached the underlying program as `--watch` and failed whenever that program claimed the option itself; arguments spelled like `pnpm run`'s own flags (`-s`, `--if-present`) were also consumed by pnpm instead of reaching the script [#13295](https://github.com/pnpm/pnpm/issues/13295). Pass those flags before the script name (`pnpm run -s test`) to apply them to pnpm.
