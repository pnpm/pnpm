---
"pnpm": patch
---

`pnpm root -g` and `pnpm bin -g` now print warnings to stderr instead of stdout, so their stdout stays a clean, machine-readable path. Previously, running either command with `--global` in a project that pins a package manager (e.g. via the `packageManager` field) printed a warning like `[WARN] Using --global skips the package manager check for this project` ahead of the path, breaking programs that capture the output as a path [#13672](https://github.com/pnpm/pnpm/issues/13672). The Rust pnpm port (pnpm 12) does not support `root -g` yet, and its `bin -g` still prints this warning to stdout — stderr routing for it will follow in a separate change.
