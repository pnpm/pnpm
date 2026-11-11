---
"pnpm": patch
"pacquet": patch
---

A root `overrides` entry that points a package at a `workspace:` path, such as `workspace:./packages/foo`, now also applies when a registry package depends on that package [#270](https://github.com/pnpm/tasks/issues/270).
