---
id: publish
title: pnpm publish
---

Publishes a package to the registry.

```sh
pnpm [-r] publish [<tarball|folder>] [--tag <tag>]
     [--access <public|restricted>] [options]
```

When publishing a package inside a [workspace](../workspaces.md), the LICENSE file
from the root of the workspace is packed with the package (unless the package
has a license of its own).

You may override some fields before publish, using the
[publishConfig] field in `package.json`.
You also can use the [`publishConfig.directory`](../package_json.md#publishconfigdirectory) to customize the published subdirectory (usually using third party build tools).

When running this command recursively (`pnpm -r publish`), pnpm will publish all
the packages that have versions not yet published to the registry.

[publishConfig]: ../package_json.md#publishconfig

## Options

### --recursive, -r

Publish all packages from the workspace.

### --json

Show information in JSON format.

### --tag &lt;tag\>

Publishes the package with the given tag. By default, `pnpm publish` updates
the `latest` tag.

For example:

```sh
# inside the foo package directory
pnpm publish --tag next
# in a project where you want to use the next version of foo
pnpm add foo@next
```

### --access &lt;public|restricted\>

Tells the registry whether the published package should be public or restricted.

### --no-git-checks

Don't check if current branch is your publish branch, clean, and up-to-date with remote.

### --publish-branch &lt;branch\>

* Default: **master** and **main**
* Types: **String**

The primary branch of the repository which is used for publishing the latest
changes.

### --force

Try to publish packages even if their current version is already found in the
registry.

### --report-summary

Save the list of published packages to `pnpm-publish-summary.json`. Useful when some other tooling is used to report the list of published packages.

An example of a `pnpm-publish-summary.json` file:

```json
{
  "publishedPackages": [
    {
      "name": "foo",
      "version": "1.0.0"
    },
    {
      "name": "bar",
      "version": "2.0.0"
    }
  ]
}
```

### --dry-run

Does everything a publish would do except actually publishing to the registry.

### --otp

When publishing packages that require two-factor authentication, this option can specify a one-time password.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)

## Configuration

You can also set `git-checks`, `publish-branch` options in the `.npmrc` file.

For example:

```ini title=".npmrc"
git-checks=false
publish-branch=production
```

## Life Cycle Scripts

* `prepublishOnly`
* `prepublish`
* `prepack`
* `prepare`
* `postpack`
* `publish`
* `postpublish`
