---
"pacquet": patch
---

`pnpm install` now fails when the project's `engines.pnpm` does not accept the running pnpm version. If the project's `engines.node` does not accept the current Node.js version, `pnpm install` prints a warning, or fails with `engineStrict` enabled [pnpm/pnpm#17751](https://github.com/pnpm/pnpm/issues/17751).
