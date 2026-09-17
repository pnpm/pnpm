---
"pacquet": minor
---

Python environments now live in the store. Each project keeps only its `.venv` link, which points at the project's current environment generation under `python-envs` in the store. A repository with many Python projects no longer holds a `.pnpm/python-envs` directory in each of them. The next install relinks a `.venv` that an earlier release published. The old `.pnpm/python-envs` directory is left in place, since a running program may still use it, and can be deleted once none does. With `frozenStore` set, pnpm writes nothing to the store, so environments stay in the project's `.pnpm/python-envs` [#15014](https://github.com/pnpm/pnpm/issues/15014).
