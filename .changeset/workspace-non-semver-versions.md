---
"@pnpm/workspace.range-resolver": patch
"pnpm": patch
"pacquet": patch
---

A `workspace:` dependency now resolves to a workspace project whose version is not valid semver, such as `1` or `1.0`. `workspace:*`, `workspace:^`, `workspace:~`, and a range identical to the version match it [#4567](https://github.com/pnpm/pnpm/issues/4567).
