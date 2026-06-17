---
"@pnpm/installing.dedupe.check": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.commands": minor
"pnpm": minor
---

Added a `--dry-run` option to `pnpm install`. It runs a full dependency resolution and reports what an install would change, but writes nothing to disk (no lockfile, no `node_modules`) and always exits with code 0. This mirrors the preview semantics of `npm install --dry-run` [#7340](https://github.com/pnpm/pnpm/issues/7340).
