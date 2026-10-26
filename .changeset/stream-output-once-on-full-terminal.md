---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
"pacquet": patch
---

Output that fills the terminal, such as `pnpm run --stream` script output, is now written once. Before, every new line also rewrote all the lines already on screen [#17387](https://github.com/pnpm/pnpm/issues/17387).
