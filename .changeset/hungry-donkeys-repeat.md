---
"pacquet": patch
---

`pnpm install` now announces `Lockfile is up to date, resolution step is skipped` whenever the headless installer runs — including installs that materialize a cold `node_modules` from an up-to-date lockfile and `--filter` subset installs — matching the TypeScript CLI. `pnpm fetch` prints `Importing packages to virtual store` on that path instead.
