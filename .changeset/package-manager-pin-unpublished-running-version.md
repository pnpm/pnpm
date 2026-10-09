---
"pacquet": patch
---

`pnpm install` no longer fails when `packageManager` pins the pnpm version that is already running and the registry does not publish that version. pnpm warns and continues. A registry mirror that has not synced a release no longer blocks the commands of a project pinned to it.
