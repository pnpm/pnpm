---
id: audit
title: pnpm audit
---

Checks for known security issues with the installed packages.

If security issues are found, try to update your dependencies via `pnpm update`.
If a simple update does not fix all the issues, use [overrides] to force
versions that are not vulnerable. For instance, if `lodash@<2.1.0` is vulnerable,
use this overrides to force `lodash@^2.1.0`:

```yaml title="pnpm-workspace.yaml"
overrides:
  "lodash@<2.1.0": "^2.1.0"
```

Or alternatively, run `pnpm audit --fix`.

If you want to tolerate some vulnerabilities as they don't affect your project, you may use the [`audit.ignore`] setting.

`pnpm audit` queries the registry's `/-/npm/v1/security/advisories/bulk` endpoint. The response does not include CVE identifiers, so advisories are identified by the GitHub advisory ID (GHSA) in their URL.

The command exits with code `1` when an advisory at or above the [audit level](#--audit-level-severity) remains after ignored advisories are removed. With `--ignore` or `--ignore-unfixable`, the command only saves the IDs and exits with code `0`.

[overrides]: ../settings/dependency-resolution.md#overrides
[`audit.ignore`]: #auditignore

## Commands

### signatures

```sh
pnpm audit signatures
```

Verifies the ECDSA registry signatures of installed packages against the public keys published by each registry at `/-/npm/v1/keys`. Scoped registries configured via [`registries`](../settings/dependency-resolution.md#registries) are respected; registries that don't publish signing keys are skipped.

Signatures are verified against the package integrity recorded in the lockfile. A package without recorded integrity cannot pass verification.

A registry signs the integrity of a package's `.tgz`. When the lockfile pins tars with [`lockfile.integrityTarget: tar`](../settings/store.md#lockfileintegritytarget), the signature is verified against the `.tgz` integrity the registry publishes, once pnpm has tied it to the tar hash in the lockfile. The store answers for a package it holds. Any other package is downloaded and checked. A package that cannot be tied is reported as invalid, with the reason.

The command exits with code `1` if any package has an invalid signature, or if a registry advertises signing keys but a package was published without a signature. Combine with `--json` to get machine-readable output.

## Options

### --audit-level &lt;severity\>

* Type: **info**, **low**, **moderate**, **high**, **critical**
* Default: **low**

Only print advisories with severity greater than or equal to `<severity>`. Advisories below this level also do not fail the command.

This can also be set via [`audit.level`](#auditlevel) in `pnpm-workspace.yaml`.

### --fix

Add overrides to the `pnpm-workspace.yaml` file to force non-vulnerable versions of the dependencies.

Use `--fix=update` to fix vulnerabilities by updating the vulnerable packages in the lockfile instead of adding overrides.

When [`minimumReleaseAge`](../settings/dependency-resolution.md#minimumreleaseage) is set, `--fix` also adds the minimum patched version of each advisory to [`minimumReleaseAgeExclude`](../settings/dependency-resolution.md#minimumreleaseageexclude) in `pnpm-workspace.yaml`, so the security fix can be installed without waiting for the release age window.

### --interactive, -i

Review the advisories selected by `--fix` and pick which ones to apply. Implies `--fix=override` when `--fix` is not given.

### --json

Output audit report in JSON format.

`patched_versions` is `null` for an advisory with no published fix, so tooling can tell "no fix available" from "fix available at version X".

### --dev, -D

Only audit dev dependencies.

### --prod, --production, -P

Only audit `dependencies` and `optionalDependencies`.

### --optional

Audit `optionalDependencies`. This is the default.

### --no-optional

Don't audit `optionalDependencies`.

### --ignore-registry-errors

If the registry responds with a non-200 status code, the process should exit with 0.
So the process will fail only if the registry actually successfully responds with found vulnerabilities.

### --ignore-unfixable

Ignore all advisories with no resolution. An unfixable advisory without a GHSA ID cannot be ignored and fails the command with `ERR_PNPM_AUDIT_MISSING_GHSA`.

Their GHSA IDs are saved in `pnpm-workspace.yaml` the same way as with [`--ignore`](#--ignore-vulnerability).

### --ignore &lt;vulnerability\>

Ignore a vulnerability by its GitHub advisory ID (GHSA).

The ID is saved in `pnpm-workspace.yaml`, which is created if missing, and no audit report is printed. It is added to [`audit.ignore`](#auditignore) if that list is already set, otherwise to the deprecated `auditConfig.ignoreGhsas`, which pnpm still reads. To have a new project write to `audit.ignore`, create the list first (`audit: { ignore: [] }`).

## Configuration

`pnpm audit` is configured in the `audit` section of `pnpm-workspace.yaml`:

```yaml title="pnpm-workspace.yaml"
audit:
  level: high
  ignore:
    - GHSA-42xw-2xvc-qx8m
```

### audit.level

* Default: **low**
* Type: **info**, **low**, **moderate**, **high**, **critical**

Only print advisories with severity greater than or equal to this level. Same as the [`--audit-level`](#--audit-level-severity) flag.

### audit.ignore

A list of GHSA codes that will be ignored by the `pnpm audit` command.

```yaml title="pnpm-workspace.yaml"
audit:
  ignore:
    - GHSA-42xw-2xvc-qx8m
    - GHSA-4w2v-q235-vp99
    - GHSA-cph5-m8f7-6c5x
    - GHSA-vh95-rmgr-6w4m
```

Since v12.4.0, ignored advisories are left out of the vulnerability
total and the per-severity counts, and reported separately. When every advisory
found is ignored, the summary says so rather than reporting a clean audit.

### audit.ignorePrune

Added in: v12.0.0

* Default: **false**
* Type: **Boolean**

When `true`, `pnpm audit --fix` removes the [`audit.ignore`](#auditignore)
entries whose GHSA no longer appears in the audit report, so a list of tolerated
advisories doesn't accumulate entries for dependencies that are long gone.

```yaml title="pnpm-workspace.yaml"
audit:
  ignorePrune: true
```

:::info

`auditLevel` and `auditConfig.ignoreGhsas` are deprecated names for `audit.level` and `audit.ignore`, and pnpm still reads them. When both spellings are set, the `audit` section takes precedence and a warning is printed.

`auditConfig.ignoreCves` is not recognized. Replace each CVE with the matching `GHSA-xxxx-xxxx-xxxx` ID, shown in the `More info` column of `pnpm audit` output, under [`audit.ignore`](#auditignore).

:::
