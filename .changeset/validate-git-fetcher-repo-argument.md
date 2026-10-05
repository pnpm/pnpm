---
"@pnpm/fetching.git-fetcher": patch
"pnpm": patch
---

pnpm now rejects a git dependency whose lockfile repository is empty, begins with `-`, or contains a null byte. Git can no longer read such a value as a command-line option [pnpm/tasks#84](https://github.com/pnpm/tasks/issues/84).
