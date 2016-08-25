# store.json

`store.json` contains information about all the different internal/external dependencies that the packages in the store have. This is especially useful because `pnpm` allows to use shared stores.

## pnpm

The last compatible pnpm version that has modified the store.

## dependents

A dictionary that shows what packages are dependent on each of the package from the store. The dependent packages can be other packages from the store, or packages that use the store to install their dependencies.

For example, `pnpm` has a dependency on `npm` and `semver`. But `semver` is also a dependency of `npm`. It means that after installation, the `store.json` would have connections like this in the `dependents` property:

```json
{
  "dependents": {
    "semver@5.3.0": [
      "/home/john_smith/src/pnpm/package.json",
      "npm@3.10.2"
    ],
    "npm@3.10.2": [
      "/home/john_smith/src/pnpm/package.json"
    ]
  }
}
```

## dependencies

A dictionary that is pretty match the opposite of `dependents`. The `store.json` from the previous example would contain the following `dependencies` property:

```json
{
  "dependencies": {
    "/home/john_smith/src/pnpm/package.json": [
      "semver@5.3.0",
      "npm@3.10.2"
    ],
    "npm@3.10.2": [
      "semver@5.3.0"
    ]
  }
}
```
