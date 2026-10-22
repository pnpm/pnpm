---
"pacquet": patch
---

`pnpm add <pkg>@<tag>` and `pnpm add <pkg>@<range>` now install the version the tag or range resolved to. Previously the install locked the newest version the saved range allowed, even when it was outside the requested range or newer than the tag [#17234](https://github.com/pnpm/pnpm/issues/17234).
