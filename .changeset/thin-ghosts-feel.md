---
"@pnpm/cli-utils": minor
"pnpm": minor
---

pnpm will now automatically load the `pnpmfile.cjs` file from any [config dependency](https://pnpm.io/config-dependencies) that is named `@pnpm/plugin-*` or `pnpm-plugin-*` [#9729](https://github.com/pnpm/pnpm/pull/9729).
