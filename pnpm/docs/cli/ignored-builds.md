---
id: ignored-builds
title: pnpm ignored-builds
---

Print the list of packages with blocked build scripts.

The command lists two groups:

* builds that the last install ignored automatically, read from `node_modules/.modules.yaml`. Before the first install, there is nothing to report;
* builds denied explicitly with `false` in [`allowBuilds`](../settings/build.md#allowbuilds).

Each package is printed under the key it takes in `allowBuilds`. To approve a build, use [`pnpm approve-builds`](./approve-builds.md).
