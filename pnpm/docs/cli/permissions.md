---
id: permissions
title: pnpm permissions
---

Added in: v12.11.0

Review what dependencies may do: run build scripts (`build`) and provide [agent skills](../agent-skills.md) (`skills`). The decisions are stored in the [`permissions`] setting of `pnpm-workspace.yaml`.

[`permissions`]: ../settings/build.md#permissions

## Commands

### pnpm permissions approve

Alias: `pnpm approve`

Approve or deny what the dependencies awaiting approval request. A package appears once, with everything it requests:

```
drizzle-kit  build, skills
esbuild      build
```

Without arguments, the packages are chosen in an interactive prompt. The packages you do not select are denied. You can also pass package names, prefixing a name with `!` to deny it:

```sh
pnpm approve drizzle-kit !esbuild
```

A named package is approved or denied for everything it requests. A package that is not awaiting approval yet is decided for every capability.

After the decision is written, pnpm rebuilds the packages approved to build and links the approved agent skills.

A `build` decision is written to `permissions` only when `pnpm-workspace.yaml` already has a `permissions` setting. Otherwise it is written to [`allowBuilds`](../settings/build.md#allowbuilds), which older pnpm versions read too.

#### --all

Approve everything awaiting approval without interactive prompts.

### pnpm permissions list

Alias: `pnpm permissions`, `pnpm permissions ls`

List the granted and denied permissions, and the packages awaiting approval.
