---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now leave out the files a manifest's `files` field excludes. A package listing `files: ["**", "!**/test"]` publishes nothing from its test directories [#15738](https://github.com/pnpm/pnpm/issues/15738).
