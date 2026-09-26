---
"pacquet": minor
---

`--allow-build` now accepts a comma-separated list of package names, so `pnpm add --allow-build=esbuild,sharp astro` allows both packages to run their build scripts. This works in `pnpm add`, `pnpm install`, `pnpm dlx`, and `pnpm create` [#11759](https://github.com/pnpm/pnpm/issues/11759).
