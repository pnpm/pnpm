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

Since v11, `pnpm audit` queries the registry's `/-/npm/v1/security/advisories/bulk` endpoint. The response does not include CVE identifiers, so advisories are filtered by GitHub advisory ID (GHSA) instead. If you previously listed CVEs under `auditConfig.ignoreCves`, replace each entry with the corresponding `GHSA-xxxx-xxxx-xxxx` value (shown in the `More info` column of `pnpm audit` output) under [`audit.ignore`].

[overrides]: ../settings/dependency-resolution.md#overrides
[`audit.ignore`]: #auditignore

## Commands

### signatures

Added in: v11.1.0

```sh
pnpm audit signatures
```

Verifies the ECDSA registry signatures of installed packages against the public keys published by each registry at `/-/npm/v1/keys`. Scoped registries configured via [`registries`](../settings/dependency-resolution.md#registries) are respected; registries that don't publish signing keys are skipped.

The command exits with code `1` if any package has an invalid signature, or if a registry advertises signing keys but a package was published without a signature. Combine with `--json` to get machine-readable output.

## Options

### --audit-level &lt;severity\>

* Type: **low**, **moderate**, **high**, **critical**
* Default: **low**

Only print advisories with severity greater than or equal to `<severity>`.

This can also be set via [`audit.level`](#auditlevel) in `pnpm-workspace.yaml`.

### --fix

Add overrides to the `pnpm-workspace.yaml` file in order to force non-vulnerable versions of the dependencies.

Use `--fix=update` (added in v11.0.0) to fix vulnerabilities by updating packages in the lockfile instead of adding overrides.

When [`minimumReleaseAge`](../settings/dependency-resolution.md#minimumreleaseage) is set, `--fix` also adds the minimum patched version of each advisory to [`minimumReleaseAgeExclude`](../settings/dependency-resolution.md#minimumreleaseageexclude) in `pnpm-workspace.yaml`, so the security fix can be installed without waiting for the release age window.

### --interactive, -i

Added in: v11.0.0

Review the advisories selected by `--fix` and pick which ones to apply. Only usable together with `--fix`.

### --json

Output audit report in JSON format.

`patched_versions` is `null` for an advisory with no published fix, so tooling can tell "no fix available" from "fix available at version X".

### --dev, -D

Only audit dev dependencies.

### --prod, -P

Only audit production dependencies.

### --no-optional

Don't audit `optionalDependencies`.

### --ignore-registry-errors

If the registry responds with a non-200 status code, the process should exit with 0.
So the process will fail only if the registry actually successfully responds with found vulnerabilities.

### --ignore-unfixable

Added in: v10.11.0

Ignore all advisories with no resolution.

Since v11, unfixable advisories are tracked by GHSA rather than CVE.

### --ignore &lt;vulnerability\>

Added in: v10.11.0

Ignore a vulnerability by its GitHub advisory ID (GHSA). Before v11 this flag accepted CVE identifiers.

## Configuration

`pnpm audit` is configured in the `audit` section of `pnpm-workspace.yaml` (added in v11.16.0):

```yaml
audit:
  level: high
  ignore:
    - GHSA-42xw-2xvc-qx8m
```

### audit.level

* Default: **low**
* Type: **low**, **moderate**, **high**, **critical**

Only print advisories with severity greater than or equal to this level. Same as the [`--audit-level`](#--audit-level-severity) flag.

### audit.ignore

A list of GHSA codes that will be ignored by the `pnpm audit` command.

```yaml
audit:
  ignore:
    - GHSA-42xw-2xvc-qx8m
    - GHSA-4w2v-q235-vp99
    - GHSA-cph5-m8f7-6c5x
    - GHSA-vh95-rmgr-6w4m
```

Since v11.26.0, ignored advisories are left out of the vulnerability
total and the per-severity counts, and reported separately. When every advisory
found is ignored, the summary says so rather than reporting a clean audit.

### audit.ignorePrune

Added in: v11.25.0

* Default: **false**
* Type: **Boolean**

When `true`, `pnpm audit --fix` removes the [`audit.ignore`](#auditignore)
entries whose GHSA no longer appears in the audit report, so a list of tolerated
advisories doesn't accumulate entries for dependencies that are long gone.

```yaml
audit:
  ignorePrune: true
```

:::info

Before v11.16.0, these settings were named `auditLevel` and `auditConfig.ignoreGhsas`. The deprecated names keep working until the next major version; when both are set, the `audit` section takes precedence and a warning is printed.

Before v11, `auditConfig.ignoreCves` was used to filter advisories by CVE identifier. That setting is no longer recognized.

:::
