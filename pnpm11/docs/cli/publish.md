---
id: publish
title: pnpm publish
---

Publishes a package to the registry.

```sh
pnpm [-r] publish [<tarball|folder>] [--tag <tag>]
     [--access <public|restricted>] [options]
```

:::note

Since v11, `pnpm publish` is implemented natively and no longer delegates to the `npm` CLI. If you rely on a feature that is now gone, please open an issue at [pnpm/pnpm](https://github.com/pnpm/pnpm/issues). As a workaround, you can still run `pnpm pack && npm publish *.tgz`.

:::

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

### --batch

Added in: v11.7.0

When publishing recursively (`pnpm -r publish`), send all selected packages to the registry in a single `PUT /-/pnpm/v1/publish` request instead of one request per package.

The target registry has to implement the batch publish endpoint ([pnpr](https://github.com/pnpm/pnpm/tree/main/pnpr) does); registries that don't are reported with an `ERR_PNPM_BATCH_PUBLISH_UNSUPPORTED` error. The batch is processed all-or-nothing: if any package in the batch fails validation, none of the packages are published.

Since v11.24.0, the selected packages are grouped by the registry each one
publishes to, and one request is sent per group. Every package in a group must
authenticate with the same credential — a scope-specific token shared across the
group is fine, and mismatched credentials for one registry are rejected *before*
anything is published, rather than after part of the release went out. The
`publish` and `postpublish` scripts run after each completed group.

### --skip-manifest-obfuscation

Added in: v11.3.0

Keep the original `packageManager` field and publish lifecycle scripts in the published manifest instead of stripping them. The pnpm-specific `pnpm` field is still omitted.

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

You can also provide the OTP via the `PNPM_CONFIG_OTP` environment variable:

```sh
export PNPM_CONFIG_OTP='<your OTP here>'
pnpm publish --no-git-checks
```

If the registry requests OTP and you have not provided it via the environment variable or the `--otp` flag, pnpm will prompt you directly for an OTP code.

If the registry requests web-based authentication, pnpm will print a scannable QR code along with the URL.

### --provenance

When publishing from a supported cloud CI/CD system, the package will be publicly linked to where it was built and published from.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)

## Configuration

You can also set `gitChecks`, `publishBranch` options in the `pnpm-workspace.yaml` file.

For example:

```yaml title="pnpm-workspace.yaml"
gitChecks: false
publishBranch: production
```

### Registry

Since v11.28.0, a scoped package is published to the registry set by `publishConfig["@<scope>:registry"]` in its `package.json`. This takes precedence over the registry set for the same scope in `.npmrc` and over `publishConfig.registry`:

```json title="package.json"
{
  "name": "@acme/utils",
  "publishConfig": {
    "@acme:registry": "https://npm.acme.example/"
  }
}
```

## Life Cycle Scripts

* `prepublishOnly`
* `prepublish`
* `prepack`
* `prepare`
* `postpack`
* `publish`
* `postpublish`
