---
"pacquet": patch
---

`pnpm version`, `pnpm add`, and `pnpm pkg set` keep JSON5 style when they update `package.json5`. Keys stay unquoted, strings keep JSON5 quotes, and trailing commas stay in place [pnpm/pnpm#15717](https://github.com/pnpm/pnpm/issues/15717).
