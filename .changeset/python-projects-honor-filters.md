---
"pacquet": patch
---

`pnpm install --filter <selector>` now installs only the Python projects the selection asks for. A Python project that shares a directory with an npm workspace project is selected with that project. A Python project in a directory of its own is selected by the distribution it declares, by its path, or through the `[tool.uv.sources]` entries that reach it. Under `--fail-if-no-match`, a selector that names only a Python project is a match. `pnpm add --filter <selector> pypi:<package>` writes the requirement to every selected project [#14945](https://github.com/pnpm/pnpm/issues/14945).
