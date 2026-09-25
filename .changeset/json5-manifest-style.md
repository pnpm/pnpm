---
"pacquet": patch
---

`pnpm version`, `pnpm add`, and `pnpm pkg set` keep JSON5 style when they update `package.json5`. ASCII identifier keys stay unquoted, strings keep JSON5 quotes, and indented files keep trailing commas [pnpm/pnpm#15717](https://github.com/pnpm/pnpm/issues/15717).
