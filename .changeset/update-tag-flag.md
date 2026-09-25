---
"pacquet": minor
---

`pnpm update` now accepts `--tag <tag>`. The flag updates the matched dependencies to the version behind the given dist-tag and rewrites the ranges in `package.json`, the way `--latest` updates to the `latest` tag. It also works with `--interactive` and with global updates [#3534](https://github.com/pnpm/pnpm/issues/3534).
