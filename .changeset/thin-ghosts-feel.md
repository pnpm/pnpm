---
"@pnpm/cli-utils": minor
"pnpm": minor
---

pnpm will now automatically load the `pnpmfile.cjs` file from any [config dependency](https://pnpm.io/config-dependencies) named `@pnpm/plugin-*` or `pnpm-plugin-*` [#9729](https://github.com/pnpm/pnpm/pull/9729).

The order in which config dependencies are initialized should not matter — they are initialized in alphabetical order. If a specific order is needed, the paths to the `pnpmfile.cjs` files in the config dependencies can be explicitly listed using the `pnpmfile` setting in `pnpm-workspace.yaml`.
