---
"@pnpm/fetching.git-fetcher": patch
"pnpm": patch
---

Reject git repository values starting with a dash or containing null bytes to prevent argument injection during git checkout [pnpm/tasks#84](https://github.com/pnpm/tasks/issues/84).
