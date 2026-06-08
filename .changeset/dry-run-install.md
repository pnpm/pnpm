---
"@pnpm/installing.commands": minor
"pnpm": minor
---

Added `--dry-run` option to `pnpm install`. Validates that the lockfile is up-to-date without installing packages. Exits with a non-zero exit code if the lockfile is outdated. Useful for pre-commit hooks to catch stale lockfiles [#7340](https://github.com/pnpm/pnpm/issues/7340).
