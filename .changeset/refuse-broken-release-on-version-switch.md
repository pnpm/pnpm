---
"pnpm": patch
"pacquet": patch
---

A project pinned to a broken pnpm release via `packageManager` or `devEngines.packageManager` now reports which release is broken and what to do about it, instead of failing inside the installer. `pnpm self-update` already refused these releases; the version switch does too.
