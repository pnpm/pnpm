---
id: init
title: "pnpm init"
---

Create a `package.json` file.

## Options

### --bare

Added in: v10.25.0

Creates a `package.json` with only the required fields.

### --init-type &lt;type\>

* Default: **module**
* Type: **commonjs**, **module**

Set the module system for the package.

### --init-package-manager

Pin the project to a pnpm version.

Since v11, the pin is written as a [`devEngines.packageManager`](../package_json.md#devenginespackagemanager) entry, so the resolved version is captured in `pnpm-lock.yaml`. Since v11.23.0 it is also written to the legacy `packageManager` field, as an exact version rather than a `^` range, because Corepack reads only `packageManager` and accepts nothing but an exact version.

Since v11.25.0, the pin can be newer than the pnpm that ran the command, so a project scaffolded by an outdated pnpm no longer inherits that staleness through its own pin ([#7490](https://github.com/pnpm/pnpm/issues/7490)). pnpm looks up the version published under the `latest` tag on the package-manager registries, and pins it **only when it is newer than the running version** — the lookup raises the pin, it never lowers it.

Two things follow from that. `latest` is a tag rather than "the newest release", so the pin tracks whichever line the tag points at: with `latest` on the pnpm 12 line, a pnpm 11.25 running `pnpm init` pins the newest pnpm 12, while a pnpm 12 newer than the `latest` tag pins itself (so does anything older than 11.25, which predates the lookup). And when the lookup cannot answer — no network, an unreachable or slow registry, `--offline`, or a `latest` that [`minimumReleaseAge`](../settings/dependency-resolution.md#minimumreleaseage) or [`trustPolicy`](../settings/dependency-resolution.md#trustpolicy) rejects — the running version is pinned, as before. The lookup never fails or hangs the command.

Inside a workspace subpackage this flag has no effect — the pin is only added to the workspace root's `package.json`, and the subpackage follows it. Pass `--no-init-package-manager` to scaffold a manifest with no pin at all.

## Configuration

These settings fill in fields of the scaffolded `package.json`. They may be set in `pnpm-workspace.yaml`, in the [global configuration file](./config.md), or through the environment as `PNPM_CONFIG_INIT_VERSION` and friends.

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
