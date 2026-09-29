---
id: self-update
title: pnpm self-update
---

Updates pnpm to the latest version or the one specified.

```
pnpm self-update [<version>]
```

Usage examples:

```
pnpm self-update
pnpm self-update 10
pnpm self-update next-10
pnpm self-update 10.6.5
```

## Behavior

The behavior of `pnpm self-update` depends on the project context:

### In a project with `managePackageManagerVersions=true`

When [`managePackageManagerVersions`](../settings.md#managepackagemanagerversions) is enabled and the project's `package.json` has a `packageManager` field set to pnpm, `self-update` only updates the `packageManager` field in `package.json` to the resolved version. It does not install pnpm globally. The next time you run a pnpm command, pnpm will automatically download and switch to the specified version.

### Without `managePackageManagerVersions` or outside a project

Otherwise, `self-update` installs the resolved pnpm version globally and links it to `PNPM_HOME` so it becomes the active pnpm binary on your system.
