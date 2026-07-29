---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Workspace installs through a pnpr server no longer crash with `Cannot read properties of undefined (reading 'filter')` after linking, when `minimumReleaseAge` is active [#13275](https://github.com/pnpm/pnpm/issues/13275).
