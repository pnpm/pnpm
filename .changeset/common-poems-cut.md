---
"@pnpm/resolve-dependencies": major
"@pnpm/headless": major
"@pnpm/deps.graph-builder": major
"@pnpm/patching.config": major
"@pnpm/patching.types": minor
"@pnpm/core": patch
"pnpm": minor
---

Add an ability to patch dependencies by version ranges. Exact versions override version ranges, which in turn override name-only patches. Version range `*` is the same as name-only, except that patch application failure will not be ignored.

For example:

```yaml
patchedDependencies:
  foo: patches/foo-1.patch
  foo@^2.0.0: patches/foo-2.patch
  foo@2.1.0: patches/foo-3.patch
```

The above configuration would apply `patches/foo-3.patch` to `foo@2.1.0`, `patches/foo-2.patch` to all `foo` versions which satisfy `^2.0.0` except `2.1.0`, and `patches/foo-1.patch` to the remaining `foo` versions.

> [!WARNING]
> The version ranges should not overlap. If you want to specialize a sub range, make sure to exclude it from the other keys. For example:
>
> ```yaml
> # pnpm-workspace.yaml
> patchedDependencies:
>   # the specialized sub range
>   'foo@2.2.0-2.8.0': patches/foo.2.2.0-2.8.0.patch
>   # the more general patch, excluding the sub range above
>   'foo@>=2.0.0 <2.2.0 || >2.8.0': 'patches/foo.gte2.patch
> ```
>
> In most cases, however, it's sufficient to just define an exact version to override the range.
