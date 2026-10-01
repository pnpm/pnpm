---
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

A filtered `pnpm run` or `pnpm exec` now finds dependencies out of date when a workspace dependency of a selected project has no `node_modules` directory, as after a filtered install. With `verifyDepsBeforeRun: install`, pnpm installs that dependency before running the command ([pnpm/tasks#45](https://github.com/pnpm/tasks/issues/45)).
