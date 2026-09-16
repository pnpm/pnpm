---
"pacquet": patch
---

`pnpm install` now installs a Python wheel whose `WHEEL` file lists tags that differ from the ones in its filename. A wheel whose filename tags were changed after the build, such as `mysql-connector-python`, was rejected [#14945](https://github.com/pnpm/pnpm/issues/14945).
