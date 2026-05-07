---
"@pnpm/deps.inspection.commands": minor
"pnpm": minor
---

Added the `pnpm bugs` command that opens a package's bug tracker URL in the browser. With no arguments, it reads the current project's `package.json`; with one or more package names, it fetches each package's metadata from the registry and opens its bug tracker. Falls back to `<repository>/issues` when the `bugs` field is missing [#11279](https://github.com/pnpm/pnpm/pull/11279).
