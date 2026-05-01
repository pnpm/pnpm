---
"pnpm": patch
---

fix(config): prevent crashes during `pnpm config set/get` by tolerating `configDependencies` install failures. When running `pnpm config`, `pnpm set` or `pnpm get`, a failure to install `configDependencies` (for example because the registry auth token has not been written yet) is now logged at debug level and the command proceeds. All other commands still surface the install error [#10684](https://github.com/pnpm/pnpm/issues/10684).
