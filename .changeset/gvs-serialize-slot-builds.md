---
"@pnpm/building.during-install": patch
"pnpm": patch
"pacquet": patch
---

Concurrent installs that share a global virtual store now run a package's build in its shared slot one at a time. A failed build leaves the slot in place and marks it for the next install to rebuild [#15568](https://github.com/pnpm/pnpm/issues/15568).
