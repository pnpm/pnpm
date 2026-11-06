---
"@pnpm/cli.utils": patch
"pnpm": patch
---

`pnpm install` now fails on a workspace project's incompatible `engines.pnpm`, or on its incompatible `engines.node` with `engineStrict` enabled, even when the project's `os`, `cpu`, or `libc` excludes the current platform [pnpm/tasks#281](https://github.com/pnpm/tasks/issues/281).
