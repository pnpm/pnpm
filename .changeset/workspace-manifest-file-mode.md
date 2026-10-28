---
"pacquet": patch
---

Files that pnpm creates in a project now get the permissions the umask gives any new file, such as `0644`. Before, several of them were readable only by the user who ran pnpm, which broke Docker images that run as a different user [#17435](https://github.com/orgs/pnpm/discussions/17435).

The fix applies to:

- `pnpm-workspace.yaml`
- the files written by `pnpm deploy`
- patch files from `pnpm patch-commit`
- the lockfile restored by `pnpm dedupe --check`
- the Cargo and Python files that pnpm writes

When pnpm edits an existing file, the file keeps its permissions.
