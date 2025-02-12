---
"@pnpm/plugin-commands-installation": minor
"pnpm": minor
---

The `pnpm add` command now supports a new flag, `--allow-build`, which allows building the specified dependencies. For instance, if you want to install a package called `bundle` that has `esbuild` as a dependency and want to allow `esbuild` to run postinstall scripts, you can run:

```
pnpm --allow-build=esbuild add bundle
```

This will run `esbuild`'s postinstall script and also add it to the `pnpm.onlyBuiltDependencies` field of `package.json`. So, `esbuild` will always be allowed to run its scripts in the future.

Related PR: [#9086](https://github.com/pnpm/pnpm/pull/9086).
