---
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

The error for a dependency with an invalid name in a project's manifest now names the project's directory. It used to say only "The current package", which did not tell which workspace project to fix [#17475](https://github.com/pnpm/pnpm/issues/17475).
