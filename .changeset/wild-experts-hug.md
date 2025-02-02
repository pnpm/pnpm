---
"@pnpm/plugin-commands-script-runners": minor
"pnpm": minor
---

Packages executed via `pnpm dlx` and `pnpm create` are allowed to be built (run postinstall scripts) by default.

If the packages executed by `dlx` or `create` have dependencies that have to be built, they should be listed via the `--allow-build` flag. For instance, if you want to run a package called `bundle` that has `esbuild` in dependencies and want to allow `esbuild` to run postinstall scripts, run:

```
pnpm --allow-build=esbuild dlx bundle
```

Related PR: [#9026](https://github.com/pnpm/pnpm/pull/9026).
