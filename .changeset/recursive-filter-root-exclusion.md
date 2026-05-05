---
"pnpm": patch
---

Fix a regression where `pnpm --recursive --filter '!<pkg>' run/exec/test/add` would include the workspace root in the matched projects. The workspace root is now correctly excluded by default when only negative `--filter` arguments are provided, matching the [documented behavior](https://pnpm.io/cli/recursive). To include the root, pass `--include-workspace-root` [#11341](https://github.com/pnpm/pnpm/issues/11341).
