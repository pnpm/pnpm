---
"pacquet": patch
---

The `pnpm` executable of the npm package now works when the package was installed without running its install scripts, as under `--ignore-scripts` or the default build-script block of pnpm and Bun [#14346](https://github.com/pnpm/pnpm/issues/14346). In that case it runs through Node.js and, in a terminal, says how to switch to the native binary.
