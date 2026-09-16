---
"pacquet": patch
---

`pnpm install` now installs a Python wheel whose `WHEEL` file lists tags that differ from the ones in its filename. Wheels that were retagged after the build, such as `mysql-connector-python`, were rejected [#14945](https://github.com/pnpm/pnpm/issues/14945).
