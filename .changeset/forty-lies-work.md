---
"@pnpm/resolve-dependencies": patch
"supi": patch
---

The lockfile should be correctly updated when a direct dependency that has peer dependencies has a new version specifier in `package.json`.

For instance, `jest@26` has `cascade@2` in its peer dependencies. So `pnpm install` will scope Jest to some version of cascade. This is how it will look like in `pnpm-lock.yaml`:

```yaml
dependencies:
  canvas: 2.6.0
  jest: 26.4.0_canvas@2.6.0
```

If the version specifier of Jest gets changed in the `package.json` to `26.5.0`, the next time `pnpm install` is executed, the lockfile should be changed to this:

```yaml
dependencies:
  canvas: 2.6.0
  jest: 26.5.0_canvas@2.6.0
```

Prior to this fix, after the update, Jest was not scoped with canvas, so the lockfile was incorrectly updated to the following:

```yaml
dependencies:
  canvas: 2.6.0
  jest: 26.5.0
```

Related issue: [#2919](https://github.com/pnpm/pnpm/issues/2919).
Related PR: [#2920](https://github.com/pnpm/pnpm/pull/2920).
