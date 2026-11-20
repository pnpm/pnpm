---
id: with
title: pnpm with
---

Run pnpm at a specific version (or the currently running one) for a single invocation, ignoring the `packageManager` and `devEngines.packageManager` fields of the project's manifest.

```sh
pnpm with <version|current> <args...>
```

The version can be an exact version, a range, or a dist-tag.

`pnpm with` does not work under Corepack. It fails with `ERR_PNPM_CANT_USE_WITH_IN_COREPACK`.

The downloaded pnpm is installed using the same mechanism as [`pnpm self-update`](./self-update.md) and cached in the global virtual store for reuse on subsequent runs.

## Examples

Run the pnpm you invoked, ignoring the version pinned in the manifest:

```sh
pnpm with current install
```

Run a specific version:

```sh
pnpm with 12.3.4 install
```

Use a dist-tag:

```sh
pnpm with next install
```

## Related settings

### pmOnFail

If you want to permanently skip the `packageManager` / `devEngines.packageManager` check (for example, because version management is handled by asdf, mise, Volta, or a similar tool), set the [`pmOnFail`](../settings/cli.md#pmonfail) setting to `ignore` instead of running every command through `pnpm with`:

```yaml title="pnpm-workspace.yaml"
pmOnFail: ignore
```
