---
"@pnpm/installing.deps-installer": patch
"@pnpm/lockfile.fs": patch
"@pnpm/pnpr.client": patch
"pnpm": patch
---

Fixed installs through a pnpr server to preserve the on-disk lockfile shape and honor frozen-lockfile controls and configured lockfile read/write settings.
