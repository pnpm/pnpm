---
id: typescript
title: Working with TypeScript
---

pnpm should work well with TypeScript out of the box most of the time.

## Do not preserve symlinks

You should not use TypeScript with [`preserveSymlinks`](https://www.typescriptlang.org/tsconfig/#preserveSymlinks) set to `true`. TypeScript will not be able to resolve the type dependencies correctly in the linked `node_modules`. If you do need to preserve symlinks for some reason, then you should set pnpm's `nodeLinker` setting to `hoisted`.

## Workspace usage

You might sometimes have issues if you have different versions of a `@types/` dependency in a workspace. These issues happen when a package requires these types without having the type dependency in dependencies. For instance, if you have `antd` in your dependencies, which relies on `@types/react`, you might get a compilation error if there are multiple versions of `@types/react` in your workspace. This is actually an issue on `antd`'s end because it should've added `@types/react` to `peerDependencies`. Luckily, you can fix this by extending `antd` with the missing peer dependency. You can do this either by adding this to your `pnpm-workspace.yaml`:

```yaml title="pnpm-workspace.yaml"
packageExtensions:
  antd:
    peerDependencies:
      '@types/react': '*'
```

Alternatively, you can install a config dependency that we created to deal with these issues [`@pnpm/plugin-types-fixer`]. Run:

```sh
pnpm add @pnpm/plugin-types-fixer --config
```

[`@pnpm/plugin-types-fixer`]: https://github.com/pnpm/plugin-types-fixer
