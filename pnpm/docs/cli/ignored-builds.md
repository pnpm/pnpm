---
id: ignored-builds
title: pnpm ignored-builds
---

Print the list of packages with blocked build scripts.

When `strictDepBuilds` is `false`, packages awaiting build approval appear in the combined "Packages awaiting approval" notice with the capabilities each package requests. When `strictDepBuilds` is `true`, the install fails with `ERR_PNPM_IGNORED_BUILDS`. This command continues to list only packages with blocked build scripts.

The command lists two groups:

* builds that the last install ignored automatically, read from `node_modules/.modules.yaml`. Before the first install, there is nothing to report;
* builds denied explicitly with `build: false` in [`permissions`](../settings/build.md#permissions) or `false` in the deprecated [`allowBuilds`](../settings/build.md#allowbuilds).

Each package is printed under the key it takes in `permissions`. To approve a build, use [`pnpm approve-builds`](./approve-builds.md).
