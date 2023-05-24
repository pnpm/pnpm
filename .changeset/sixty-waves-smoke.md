---
"@pnpm/config": major
"@pnpm/core": minor
"pnpm": minor
---

This is a semi-breaking change as the default value of the `resolution-mode` setting is changed to `highest`. This change has been requested by template authors.

When running install on a project without a lockfile, the dependencies of the project will be updated to the latest versions that satisfy the ranges in `package.json`.

Related PR: [#6575](https://github.com/pnpm/pnpm/pull/6575).
