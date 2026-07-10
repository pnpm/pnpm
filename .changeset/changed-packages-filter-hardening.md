---
"@pnpm/workspace.projects-filter": patch
"pnpm": patch
---

The changed-packages filter (`--filter "...[<since>]"`) no longer allows an option-like `<since>` value (such as `--output=<path>`) to be interpreted as a git option — git now rejects it as a bad revision. The repository root is also resolved to the nearest `.git` entry, so the filter works in a git worktree checked out inside another repository's tree.
