---
"pacquet": patch
---

Fixed repeat installs paying for a full lockfile comparison forever after a modification-time collision. When a `package.json` was last modified inside the same clock tick that the install recorded as its validation baseline — a fast install, a checkout that copied files with identical timestamps, or any filesystem that keeps only whole-second modification times — the manifest kept reading as possibly-modified, so every later `pnpm install` and `verify-deps-before-run` check re-compared the manifests against the lockfile instead of taking the fast path [#13907](https://github.com/pnpm/pnpm/issues/13907).
