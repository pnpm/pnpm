---
"pnpm": patch
---

`pnpm root -g` and `pnpm bin -g` now print warnings to stderr instead of stdout, so their stdout stays a clean, machine-readable path. Previously, running either command with `--global` in a project that pins a package manager (e.g. via the `packageManager` field) printed a warning like `[WARN] Using --global skips the package manager check for this project` ahead of the path, breaking programs that capture the output as a path [#13672](https://github.com/pnpm/pnpm/issues/13672).
