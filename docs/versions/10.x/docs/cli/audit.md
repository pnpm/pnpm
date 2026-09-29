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

If you want to tolerate some vulnerabilities as they don't affect your project, you may use the [`auditConfig.ignoreCves`] setting.

[overrides]: ../settings.md#overrides
[`auditConfig.ignoreCves`]: ../settings.md#auditconfigignorecves

## Options

### --audit-level &lt;severity\>

* Type: **low**, **moderate**, **high**, **critical**
* Default: **low**

Only print advisories with severity greater than or equal to `<severity>`.

This can also be set via `auditLevel` in `pnpm-workspace.yaml`.

### --fix

Add overrides to the `package.json` file in order to force non-vulnerable versions of the dependencies.

### --json

Output audit report in JSON format.

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

Ignore all CVEs with no resolution.

### --ignore &lt;vulnerability\>

Added in: v10.11.0

Ignore a vulnerability by CVE.

## Configuration

### auditConfig

#### auditConfig.ignoreCves

A list of CVE IDs that will be ignored by the [`pnpm audit`] command.

```yaml
auditConfig:
  ignoreCves:
    - CVE-2022-36313
```

[`pnpm audit`]: #

#### auditConfig.ignoreGhsas

A list of GHSA Codes that will be ignored by the [`pnpm audit`] command.

```yaml
auditConfig:
  ignoreGhsas:
    - GHSA-42xw-2xvc-qx8m
    - GHSA-4w2v-q235-vp99
    - GHSA-cph5-m8f7-6c5x
    - GHSA-vh95-rmgr-6w4m
```
