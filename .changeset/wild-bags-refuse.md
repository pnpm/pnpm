---
"@pnpm/core": minor
"@pnpm/headless": minor
"@pnpm/plugin-commands-installation": minor
"pnpm": minor
---

New property supported via the `dependenciesMeta` field of `package.json`: `injected`. When `injected` is set to `true`, the package will be hard linked to `node_modules`, not symlinked [#3915](https://github.com/pnpm/pnpm/pull/3915).

For instance, the following `package.json` in a workspace will create a symlink to `bar` in the `node_modules` directory of `foo`:

```json
{
  "name": "foo",
  "dependencies": {
    "bar": "workspace:1.0.0"
  }
}
```

But what if `bar` has `react` in its peer dependencies? If all projects in the monorepo use the same version of `react`, then no problem. But what if `bar` is required by `foo` that uses `react` 16 and `qar` with `react` 17? In the past, you'd have to choose a single version of react and install it as dev dependency of `bar`. But now with the `injected` field you can inject `bar` to a package, and `bar` will be installed with the `react` version of that package.

So this will be the `package.json` of `foo`:

```json
{
  "name": "foo",
  "dependencies": {
    "bar": "workspace:1.0.0",
    "react": "16"
  },
  "dependenciesMeta": {
    "bar": {
      "injected": true
    }
  }
}
```

`bar` will be hard linked into the dependencies of `foo`, and `react` 16 will be linked to the dependencies of `foo/node_modules/bar`.

And this will be the `package.json` of `qar`:

```json
{
  "name": "qar",
  "dependencies": {
    "bar": "workspace:1.0.0",
    "react": "17"
  },
  "dependenciesMeta": {
    "bar": {
      "injected": true
    }
  }
}
```

`bar` will be hard linked into the dependencies of `qar`, and `react` 17 will be linked to the dependencies of `qar/node_modules/bar`.
