---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

When another workspace project adds a different version of a package, a project's auto-installed peer on that package now moves to the version a fresh install picks. It used to stay on its locked version while any other project still accepted that version [#17058](https://github.com/pnpm/pnpm/issues/17058).
