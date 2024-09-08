---
"@pnpm/plugin-commands-listing": minor
"@pnpm/reviewing.dependencies-hierarchy": minor
"@pnpm/list": minor
"pnpm": minor
---

Support for a new CLI flag, `--exclude-peers`, added to the `list` and `why` commands. When `--exclude-peers` is used, peer dependencies are not printed in the results, but dependencies of peer dependencies are still scanned [#8506](https://github.com/pnpm/pnpm/pull/8506).
