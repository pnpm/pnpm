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

When publishing a folder or a tarball, pnpm includes a package-root README in
the registry metadata if the manifest has no `readme` value. It recognizes
`README.md`, other README files with npm-compatible Markdown extensions (such
as `README.markdown` and `README.mdown`), and `README`. It prefers `README.md`,
then another Markdown README, then `README`.

When publishing a pre-built tarball, pnpm rejects manifests, README files, and
archive metadata larger than 64 MiB.

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

### --publish-wait-timeout &lt;milliseconds\>

Added in: v12.7.0

* Default: **0**
* Type: **Number** (non-negative integer)

Wait for the exact published version to appear in the registry's install metadata.
Then check that a `HEAD` request to its tarball URL returns HTTP `200`.
This helps release workflows handle registry delays, such as npm's [publish-time malware scanning](https://github.blog/changelog/2026-07-28-npm-publish-time-malware-scanning-and-dual-use-metadata/).
A value of `0` disables the check.

```sh
pnpm publish --publish-wait-timeout 600000
pnpm -r publish --publish-wait-timeout 600000 --report-summary
```

The timeout is in milliseconds and includes registry requests and retry delays.
Each package has its own timeout, including selected versions that already exist.
For a new upload, the timeout starts after the registry accepts it.
For an existing version, it starts when pnpm begins its availability check.
With `--batch`, each uploaded registry group shares one timeout.

Recursive publishing also checks selected versions that already exist in the
registry. Without `--batch`, pnpm confirms availability before publishing dependent
packages. With `--batch`, pnpm checks each registry group after its upload.
The `publish` and `postpublish` scripts run after the corresponding check succeeds.

If confirmation times out, the command fails with
`ERR_PNPM_PUBLISH_AVAILABILITY_TIMEOUT`. The upload remains accepted and may
become available later. Do not publish the same version again.
Use [`--report-summary`](#--report-summary) with recursive publishing to record
accepted uploads even if the command fails.

Invalid registry responses or non-retryable HTTP errors, such as authentication
failures, cause `ERR_PNPM_PUBLISH_AVAILABILITY_CHECK_FAILED`.

`--dry-run` skips availability checks. `pnpm stage publish` ignores the configured
default and rejects an explicit positive `--publish-wait-timeout`, because
staged versions cannot be installed.
The check does not download the archive or install the package. It does not
verify the archive's contents.

Use [`publishWaitTimeout`](#publishwaittimeout) to set a default.

### --report-summary

Save the list of published packages to `pnpm-publish-summary.json`. Useful when some other tooling is used to report the list of published packages.

An example of a `pnpm-publish-summary.json` file:

```json title="pnpm-publish-summary.json"
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

When a recursive publish fails after the registry accepted some of the
uploads, the summary still lists the packages that were published.

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

You can also set `gitChecks`, `publishBranch`, and `publishWaitTimeout` options in the `pnpm-workspace.yaml` file.

For example:

```yaml title="pnpm-workspace.yaml"
gitChecks: false
publishBranch: production
```

### publishWaitTimeout

Added in: v12.7.0

* Default: **0**
* Type: **Number** (non-negative integer)

Sets the default timeout in milliseconds for
[`--publish-wait-timeout`](#--publish-wait-timeout-milliseconds).
Configure it in `pnpm-workspace.yaml` or the global `config.yaml`:

```yaml title="pnpm-workspace.yaml"
publishWaitTimeout: 600000
```

The `PNPM_CONFIG_PUBLISH_WAIT_TIMEOUT` environment variable overrides the
configuration files. The command-line option takes precedence over both,
including `--publish-wait-timeout=0` to disable waiting for one command.

## Life Cycle Scripts

* `prepublishOnly`
* `prepublish`
* `prepack`
* `prepare`
* `postpack`
* `publish`
* `postpublish`
