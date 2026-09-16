---
"pacquet": patch
---

`pnpm add pypi:<package>` in a directory that has no `pyproject.toml` now names the missing file and says where to run the command. It used to fail with a bare `No such file or directory (os error 2)` [#14945](https://github.com/pnpm/pnpm/issues/14945).
