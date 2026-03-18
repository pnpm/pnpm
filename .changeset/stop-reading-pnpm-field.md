---
"@pnpm/config.reader": major
"@pnpm/config.writer": major
"@pnpm/building.after-install": major
"@pnpm/installing.commands": major
"@pnpm/deps.status": major
"@pnpm/releasing.commands": major
"@pnpm/workspace.projects-reader": major
"pnpm": major
---

pnpm no longer reads settings from the `pnpm` field of `package.json`. All settings must now be configured in `pnpm-workspace.yaml` [#10086](https://github.com/pnpm/pnpm/pull/10086).

**Migration:** Move any settings from the `pnpm` field in your root `package.json` to `pnpm-workspace.yaml`. For example:

```json
// package.json (old)
{
  "pnpm": {
    "overrides": { "foo": "1.0.0" },
    "patchedDependencies": { "bar@1.0.0": "patches/bar@1.0.0.patch" }
  }
}
```

```yaml
# pnpm-workspace.yaml (new)
overrides:
  foo: "1.0.0"
patchedDependencies:
  bar@1.0.0: patches/bar@1.0.0.patch
```

Additional changes:
- `getOptionsFromRootManifest` has been removed from `@pnpm/config.reader`'s public API.
- The `pnpm unlink` command now reads and writes overrides from/to `pnpm-workspace.yaml` instead of `package.json`.
- Patch file paths in `patchedDependencies` are now resolved to absolute paths at configuration load time.
- Warnings about unsupported `pnpm.*` fields in non-root project `package.json` files have been removed.
