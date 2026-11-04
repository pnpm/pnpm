---
id: publish
title: pnpm publish
---

Publishes a package to the registry.

```sh
pnpm [-r] publish [<tarball|folder>] [--tag <tag>]
     [--access <public|restricted>] [options]
```

When publishing a folder or a tarball, pnpm includes a package-root README in
the registry metadata if the manifest has no `readme` value. It recognizes
`README.md`, other README files with npm-compatible Markdown extensions (such
as `README.markdown` and `README.mdown`), and `README`. It prefers `README.md`,
then another Markdown README, then `README`. This is separate from
[`--embed-readme`](#--embed-readme), which embeds the README in the published
manifest.

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

Skip the git checks. By default, pnpm checks that the working tree is clean, that the current branch is the publish branch, and that the branch is up to date with the remote.

### --publish-branch &lt;branch\>

* Default: **master** or **main**
* Types: **String**

The branch that publishing is allowed from. It overrides the
[`publishBranch`](#publishbranch) setting. Without either, the current branch
must be `master` or `main`. In CI, a detached `HEAD` skips the branch check.

### --ignore-scripts

Don't run the publish lifecycle scripts.

### --embed-readme

* Default: **false**
* Type: **Boolean**

Embed the README contents in the published manifest. The `embedReadme` setting
sets the default.

### --force

Try to publish packages even if their current version is already found in the
registry.

### --batch

When publishing recursively (`pnpm -r publish`), send all selected packages to the registry in a single `PUT /-/pnpr/v0/publish` request instead of one request per package.

The target registry has to implement the batch publish endpoint ([pnpr](https://github.com/pnpm/pnpm/tree/main/pnpr) does); registries that don't are reported with an `ERR_PNPM_BATCH_PUBLISH_UNSUPPORTED` error. The batch is processed all-or-nothing: if any package in the batch fails validation, none of the packages are published.

A package that declares [`publishConfig.variants`](../package_json.md#publishconfigvariants) can be published in a batch to a registry declared with [`serverType: pnpr`](../registries.md#servertype). In such a batch, pnpm first uploads the archives it packed for every package to the registry's staging area, and checks that the registry still holds each archive another job uploaded with [`--upload-variant`](#--upload-variant-target). The batch then references all of them by integrity. One batch references at most 256 archives per registry; a batch with variants that needs more fails with `ERR_PNPM_BATCH_PUBLISH_TOO_MANY_ARCHIVES`.

The selected packages are grouped by the registry each one
publishes to, and one request is sent per group. Every package in a group must
authenticate with the same credential. A scope-specific token shared across the
group is fine, and mismatched credentials for one registry are rejected *before*
anything is published, rather than after part of the release went out. The
`publish` and `postpublish` scripts run after each completed group.

### --no-variants

Publish only the archive that targets `any` of a package that declares [`publishConfig.variants`](../package_json.md#publishconfigvariants). Without it, pnpm refuses to publish such a package to a registry that is not declared with `serverType: pnpr`, because a version published without its variants can't get them later.

### --upload-variant &lt;target\>

Pack the archive of the [`publishConfig.variants`](../package_json.md#publishconfigvariants) entry that lists this target, upload it to the registry's staging area, and print `<target>=<integrity>`. Nothing is published: a later `pnpm publish --variant-integrity` publishes the version with it. Can be repeated, and `any` uploads the archive of every other host.

This lets each job of a CI matrix upload the archive it built. Every job must publish as the same account, which trusted publishing gives every job of a workflow. Uploading the same archive again renews it. The registry keeps an uploaded archive for a day.

```sh
value=$(pnpm publish --upload-variant linux-x64-musl)
```

With `--json`, pnpm prints the package's name and version, and each archive's tag, targets, and integrity.

### --variant-integrity &lt;target\>=&lt;integrity\>

Publish the version from archives other jobs uploaded with `--upload-variant`, given by the values they printed. Repeat it once per entry of `publishConfig.variants`, the `any` entry included, naming each by any one of its targets. With `--recursive`, name the package first: `<name>~<target>=<integrity>`.

pnpm packs none of the archives of a package that a value names. Before publishing, it checks that every entry has exactly one value and that the registry still holds each archive for this account. It fails naming the archive whose upload job has to run again. With `--recursive`, a selected package that no value names is packed and published as usual, and each value must name exactly one selected, publishable project.

### --release-notes &lt;file\>

A file of release notes to send to a registry declared with [`serverType: pnpr`](../registries.md#servertype). The publish fails when the file does not exist. Publishing one package, the notes are its version notes. Publishing several packages with `--batch`, they are the notes of the whole release.

In a workspace that uses [pnpm's versioning](../versioning.md#release-notes-on-pnpr), give the summary to `pnpm version -r --release-notes` instead: `pnpm version -r` composes every note of the release, and `pnpm publish` fails with `ERR_PNPM_RELEASE_NOTES_FROM_VERSIONING` when `--release-notes` is given for packages it composed notes for.

Without this option, pnpm sends each package's notes for the version it publishes:

- In a workspace that uses [pnpm's versioning](../versioning.md#release-notes-on-pnpr), the notes composed from its change intents.
- Otherwise, the section of the package's `CHANGELOG.md` under the heading of the version, as changesets (`## 1.4.0`) and Keep a Changelog (`## [1.4.0] - 2026-10-28`) write it, without the heading itself.

pnpm prints where each package's notes came from. Notes are at most 64 KiB and may not contain control characters other than line feed and tab, or bidirectional override characters. pnpm checks this before anything is uploaded. Registries not declared with `serverType: pnpr` receive no notes.

### --no-release-notes

Send no release notes.

### --publication-name &lt;name\>

The name of the release that `--batch` publishes to a registry declared with `serverType: pnpr`, such as `v12.0.0`. It may contain letters, digits, `.`, `_`, `+`, and `-`, is at most 214 bytes long, and is not a number, since a number names a release by its identifier. In a workspace that uses pnpm's versioning, the release is named after the version of its epic lead or its one fixed group by default.

### --skip-manifest-obfuscation

Keep the original `packageManager` field and publish lifecycle scripts in the published manifest instead of stripping them. The `pnpm` field is still omitted, except for `pnpm.config`.

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

Under trusted publishing, pnpm attaches provenance to a public package from a public repository on its own.
Use `--no-provenance` (or `--provenance=false`) to publish without it, for example from a self-hosted runner.
Use [`provenance`](#provenance) to set a default.

For a package with [`publishConfig.variants`](../package_json.md#publishconfigvariants), one statement names every archive the publish supplies: the archive that targets `any` and each platform variant, including those other jobs uploaded with `--upload-variant`. A version without an `any` archive gets one that the registry generates for npm clients, and the statement does not name it. `--upload-variant` itself does not sign provenance.

Running a publish to a registry declared with [`serverType: pnpr`](../registries.md#servertype) again is safe, with or without provenance. A rerun that signs provenance again sends a new statement, so the registry answers that the version is already published. pnpm then reports the version as published only when all of these hold:

- the version's release record names the same archives, the archive that targets `any` and each platform variant;
- when the rerun signs provenance, the version has a statement naming every one of those archives;
- the dist-tag the publish sets points at the version.

Otherwise it fails with `ERR_PNPM_PUBLISHED_RELEASE_DIFFERS` and names what differs. A version without an `any` archive is compared by its variants only.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)

## Configuration

You can also set `gitChecks`, `publishBranch`, `publishWaitTimeout`, and `provenance` options in the `pnpm-workspace.yaml` file.

For example:

```yaml title="pnpm-workspace.yaml"
gitChecks: false
publishBranch: production
```

### publishBranch

* Default: **master** or **main**
* Type: **String**

Sets the default for [`--publish-branch`](#--publish-branch-branch).
Configure it in `pnpm-workspace.yaml`.

The `PNPM_CONFIG_PUBLISH_BRANCH` environment variable overrides
`pnpm-workspace.yaml`. The command-line option takes precedence over both.

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

### provenance

* Default: **undefined**
* Type: **Boolean**

Sets the default for [`--provenance`](#--provenance).
When unset, trusted publishing decides.
Set it to `false` to never attach provenance:

```yaml title="pnpm-workspace.yaml"
provenance: false
```

The `PNPM_CONFIG_PROVENANCE` environment variable overrides the
configuration files. The command-line option takes precedence over both.

## Life cycle scripts

* `prepublishOnly`
* `prepublish`
* `prepack`
* `prepare`
* `postpack`
* `publish`
* `postpublish`
