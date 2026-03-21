---
"@pnpm/installing.commands": minor
"pnpm": minor
---

Add protocol selection flags to `pnpm link`:

- `--file` / `-f` saves linked dependencies as `file:` dependency specifiers
- `--link` / `-l` saves linked dependencies as `link:` dependency specifiers (default)

By default, `pnpm link` behavior remains unchanged and still uses `link:`.
