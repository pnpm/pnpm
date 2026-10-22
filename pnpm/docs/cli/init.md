---
id: init
title: "pnpm init"
---

Create a `package.json` file. If `package.json` already exists, the command fails with `ERR_PNPM_PACKAGE_JSON_EXISTS`.

```sh
pnpm init
```

## Options

### --bare

Creates a `package.json` with only the required fields.

### --init-type &lt;type\>

* Default: **module**
* Type: **commonjs**, **module**

Set the module system for the package. Set [`initType`](#inittype) to change the default.

### --init-package-manager

* Default: **true**

Pin the project to a pnpm version. Set [`initPackageManager`](#initpackagemanager) to change the default.

The pin is written as a [`devEngines.packageManager`](../package_json.md#devenginespackagemanager) entry, so the resolved version is captured in `pnpm-lock.yaml`. It is also written to the `packageManager` field as an exact version, because Corepack reads only `packageManager` and accepts nothing but an exact version.

pnpm looks up the version published under the `latest` tag on the package-manager registries, and pins it **only when it is newer than the running version**. Otherwise the running version is pinned. The lookup raises the pin, it never lowers it.

`latest` is a tag rather than "the newest release", so the pin tracks whichever line the tag points at. A pnpm newer than the `latest` tag pins itself. When the lookup cannot answer (no network, an unreachable or slow registry, `--offline`, or a `latest` that [`minimumReleaseAge`](../settings/dependency-resolution.md#minimumreleaseage) or [`trustPolicy`](../settings/dependency-resolution.md#trustpolicy) rejects), the running version is pinned. The lookup never fails or hangs the command.

Inside a workspace subpackage this flag has no effect. The pin is only added to the workspace root's `package.json`, and the subpackage follows it. Pass `--no-init-package-manager` to scaffold a manifest with no pin at all.

## Configuration

These settings fill in fields of the scaffolded `package.json`. They may be set in `pnpm-workspace.yaml`, in the [global configuration file](./config.md), or through the environment as `PNPM_CONFIG_INIT_VERSION` and friends.

### initPackageManager

* Default: **true**
* Type: **Boolean**

Whether `pnpm init` pins the project to a pnpm version. Same as the [`--init-package-manager`](#--init-package-manager) flag.

### initType

* Default: **module**
* Type: **commonjs**, **module**

The module system written into the new `package.json`. Same as the [`--init-type`](#--init-type-type) flag.

### initVersion

* Default: **1.0.0**
* Type: **String**

The `version` written into the new `package.json`.

### initLicense

* Default: **ISC**
* Type: **String**

The `license` written into the new `package.json`.

### initAuthorName, initAuthorEmail, initAuthorUrl

* Type: **String**

Assembled into the `author` field, in npm's `Name <email> (url)` form. A field left unset is left out of the string; with none of the three set, no `author` field is written.

```yaml title="pnpm-workspace.yaml"
initLicense: MIT
initAuthorName: Zoltan Kochan
initAuthorUrl: https://kochan.io
```
