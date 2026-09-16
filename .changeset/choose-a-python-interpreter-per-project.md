---
"pacquet": minor
---

`pnpm install` now chooses a Python interpreter for each project instead of installing every project with one interpreter [#14945](https://github.com/pnpm/pnpm/issues/14945). A project is installed with the first interpreter on the machine that its `requires-python` accepts, so a workspace can hold projects that support different Python versions. pnpm reads `.python-version` too, and prefers the version it asks for. Set `python.executable` in `pnpm-workspace.yaml` to name one interpreter for every project.
