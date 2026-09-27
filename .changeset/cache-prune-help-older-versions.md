---
"pacquet": patch
---

`pnpm cache prune --help` now says that pnpm 11.26 and earlier, and pnpm 12.3 and earlier, depend on the directories it removes. Offline installs with those versions fail until they refetch registry metadata [#15656](https://github.com/pnpm/pnpm/issues/15656).
