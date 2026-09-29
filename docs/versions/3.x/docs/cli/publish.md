---
id: publish
title: pnpm publish
original_id: publish
---

Publishes a package to the registry.

```text
pnpm publish [<tarball>|<folder>] [--tag <tag>] [--access <public|restricted>]
```

When publishing a package inside a [workspace](../workspaces.md), the LICENSE file from the
root of the workspace is packed with the package (unless the package has a license of its own).

You may override some fields before publish, using the [publishConfig](../package_json.md#publishconfig)
field in `package.json`.

## Options

### --tag &lt;tag>

Publishes the package with the given tag. By default, `pnpm publish` updates the `latest` tag.

For example:

```sh
# inside the foo package directory
pnpm publish --tag next
# in a project where you want to use the next version of foo
pnpm add foo@next
```

### --access &lt;public|restricted>

Tells the registry whether the published package should be public or restricted.
