---
"pacquet": patch
---

Commands that do not use the store no longer create a temporary file in the project directory when they load their settings. These include `pnpm view`, `pnpm config`, `pnpm root`, `pnpm bin`, `pnpm exec`, `pnpm run`, the script shortcuts such as `pnpm test`, and the registry commands such as `pnpm whoami`, `pnpm dist-tag`, and `pnpm search`. `pnpm exec`, `pnpm run`, and the script shortcuts still create one in projects that declare `configDependencies`.
