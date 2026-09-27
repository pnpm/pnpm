---
"pnpm": patch
"pacquet": patch
---

Settings given on the command line, such as `--registry` and `--store-dir`, now take precedence over the values a pnpmfile `updateConfig` hook sets [#14063](https://github.com/pnpm/pnpm/issues/14063).
