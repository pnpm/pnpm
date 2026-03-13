---
"@pnpm/filter-workspace-packages": minor
---

Drop `directory` as required filetype for `findUp` to allow git-based filtering to work inside git worktrees, which store `.git` as a file rather than directory.
