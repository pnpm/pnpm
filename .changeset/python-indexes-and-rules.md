---
"pacquet": minor
---

Python installs can resolve from several indexes. A `registries` entry that names `ecosystem: pypi` declares one, and they are searched in the order declared.

`python.overrides` and `python.constraints` pin the versions a resolution may pick. pnpm reads uv's own overrides and constraints from `pyproject.toml` too [pnpm/pnpm#14945](https://github.com/pnpm/pnpm/issues/14945).
