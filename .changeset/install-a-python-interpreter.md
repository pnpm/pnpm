---
"pacquet": minor
---

`pnpm install` now installs a Python interpreter when no interpreter on the machine fits the project [#14945](https://github.com/pnpm/pnpm/issues/14945). The builds are [python-build-standalone](https://github.com/astral-sh/python-build-standalone)'s, which uv and rye install too. One interpreter is shared by every project on the machine, and a later install uses it without downloading anything. `runtimeOnFail` decides what an install with no interpreter that fits does, the way it does for a Node.js runtime. `error` reports the project instead of installing one. `warn` and `ignore` install with an interpreter the machine has that the project's `requires-python` rejects. `python.downloadUrl` names a mirror.
