---
"@pnpm/parse-cli-args": patch
"pnpm": patch
---

Fixed `--prefix=<dir>` not being honored when locating the workspace root. The `--prefix → dir` rename was applied after workspace detection, so workspace settings declared in `<dir>/pnpm-workspace.yaml` were not loaded when pnpm was invoked from outside `<dir>` [#11535](https://github.com/pnpm/pnpm/issues/11535).
