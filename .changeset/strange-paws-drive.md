---
"@pnpm/plugin-commands-installation": major
"pnpm": major
---

Throw an error if `pnpm update --latest` runs with arguments containing versions specs. For instance, `pnpm update --latest foo@next` is not allowed [#7567](https://github.com/pnpm/pnpm/pull/7567).
