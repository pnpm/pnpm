---
"pnpm": patch
"pacquet": patch
---

Global commands such as `pnpm add --global`, `pnpm list --global`, and `pnpm bin --global` now run with the pnpm you invoked, even in a project that pins another pnpm version. Previously, a pin with `onFail: "download"` switched them to the pinned pnpm, and a pinned pnpm 10 or older failed because its global bin directory was not in `PATH` [#14531](https://github.com/pnpm/pnpm/issues/14531).
