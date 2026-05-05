---
"pnpm": patch
---

Prevent crashes during `pnpm config`, `pnpm set`, and `pnpm get` by tolerating `configDependencies` install failures. For these commands, a failure to install `configDependencies` (for example because the registry auth token has not been written yet) is now logged at debug level and the command proceeds. All other commands still surface the install error [#10684](https://github.com/pnpm/pnpm/issues/10684).
