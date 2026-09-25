---
"@pnpm/worker": patch
"pnpm": patch
---

On Windows, a dependency's build script now runs on every install when the build changes nothing inside the package directory, such as a script that installs git hooks [#15667](https://github.com/pnpm/pnpm/issues/15667).
