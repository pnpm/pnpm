---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Restore the peer suffix encoding used by pnpm 10 for linked dependency paths. A `filenamify` upgrade changed how leading `./` and `../` segments were normalized, producing peer suffixes like `(b@+packages+b)` instead of `(b@packages+b)` for linked packages outside the workspace root, causing lockfile churn [#11272](https://github.com/pnpm/pnpm/issues/11272).
