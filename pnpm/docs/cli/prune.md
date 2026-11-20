---
id: prune
title: pnpm prune
---

Removes extraneous packages from `node_modules`: packages that are no longer in the lockfile.

```sh
pnpm prune
```

## Options

### --prod, -P, --production

Remove the packages specified in `devDependencies`.

### --dev, -D

Keep only the packages specified in `devDependencies`.

### --no-optional

Remove the packages specified in `optionalDependencies`.

### --ignore-scripts

Don't run lifecycle scripts. `--no-ignore-scripts` runs them even when the `ignoreScripts` setting is `true`.

### --trust-lockfile

Added in: v12.15.0

Skip verifying the lockfile against the supply-chain policies. `pnpm prune` also honors [`trustLockfile`](../settings/dependency-resolution.md#trustlockfile) from the configuration. `--no-trust-lockfile` verifies the lockfile even when the configuration trusts it.

### --recursive, -r

Run `pnpm prune` in every package found in subdirectories, or in every workspace package when executed inside a workspace. Running `pnpm prune` at the workspace root runs recursively across the workspace by default.

### --filter &lt;package_selector&gt;

Restrict `pnpm prune` to the selected workspace projects. [Read more about filtering.](../filtering.md)
