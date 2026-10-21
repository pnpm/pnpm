---
"pacquet": patch
---

Fixed `pnpm install` and `pnpm peers check` missing peer dependency issues in linked projects that use `package.json5` or `package.yaml`. Linked package versions are now reported correctly for these formats [pnpm/tasks#172](https://github.com/pnpm/tasks/issues/172).
