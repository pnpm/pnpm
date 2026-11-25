---
"pnpm": patch
---

`pnpm install` no longer warns about a project's own `pnpm-workspace.yaml` when the workspace root already sets every setting in it. A project that is also installed on its own, such as a git submodule, can keep its `pnpm-workspace.yaml` without the warning [#18441](https://github.com/orgs/pnpm/discussions/18441).
