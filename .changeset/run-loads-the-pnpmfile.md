---
"pacquet": patch
---

Fixed `pnpm run`, `pnpm exec`, `pnpm rebuild`, and the script shortcuts (`pnpm test`, `pnpm start`, `pnpm stop`, `pnpm restart`, `pnpm <script>`) not loading the pnpmfile, so an `updateConfig` hook never applied to them [#14433](https://github.com/pnpm/pnpm/issues/14433). A hook's settings — `extraEnv` and `extraBinPaths` among them — now reach the scripts and commands these spawn, as they do on pnpm 11.
