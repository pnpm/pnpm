# store.yaml

`store.yaml` contains information about all the different internal/external dependencies that the packages in the store have. This is especially useful because `pnpm` allows to use shared stores.

## pnpm

The last compatible pnpm version that has modified the store.

## packages[packageId].dependents

A dictionary that shows what packages are dependent on each of the package from the store. The dependent packages can be other packages from the store, or packages that use the store to install their dependencies.

For example, `pnpm` has a dependency on `npm` and `semver`. But `semver` is also a dependency of `npm`. It means that after installation, the `store.yaml` would have connections like this in the `dependents` property:

```yaml
packages:
  semver@5.3.0: 
    dependents:
      - /home/john_smith/src/pnpm
      - npm@3.10.2
  npm@3.10.2:
    dependents:
      - /home/john_smith/src/pnpm
```

## packages[packageId].dependencies

A dictionary that is the opposite of `dependents`. However, it contains not just a list of dependency names but a map of the dependencies to their exact resolved ID.

```yaml
packages:
  /home/john_smith/src/pnpm:
    dependencies:
      semver: semver@5.3.0
      npm: npm@3.10.2
  npm@3.10.2:
    dependencies:
      semver: semver@5.3.0
```
