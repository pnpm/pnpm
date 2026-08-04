---
"@pnpm/config.reader": minor
"pnpm": minor
"pacquet": minor
---

A `scope` set in a project's `pnpm-workspace.yaml` is now ignored, with a warning. `pnpm login` records the scope as a `@scope:registry` route in the global `auth.ini`, which outranks `~/.npmrc` in every project on the machine — so a repository-committed file could redirect a scope such as `@acme` to the public registry for all of the user's unrelated projects after one routine login. The scope is now read only from `--scope`, the `PNPM_CONFIG_SCOPE` environment variable, and the global config file [#13557](https://github.com/pnpm/pnpm/issues/13557).
