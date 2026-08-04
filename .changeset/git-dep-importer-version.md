---
"pacquet": patch
---

A git-hosted dependency with no host archive (an ssh, self-hosted, or `git+file:` repo) whose package name matches the dependency's alias now records the bare `git+<repo>#<commit>` reference in the lockfile's importer entry, matching pnpm's `pnpm-lock.yaml` output instead of prefixing it with `<name>@`.
